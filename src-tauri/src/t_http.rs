//! Loopback HTTP transport for original images and Range-capable video playback.
use once_cell::sync::OnceCell;
use std::io::{self, SeekFrom};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, UNIX_EPOCH};
use tauri::Manager;
use tokio::io::{AsyncReadExt, AsyncSeekExt, AsyncWriteExt};
use tokio::net::TcpStream;

static ORIGINAL_MEDIA_BASE_URL: OnceCell<String> = OnceCell::new();

pub fn init_original_media_server(app: &tauri::AppHandle) -> io::Result<()> {
    let runtime = tauri::async_runtime::handle();
    let _runtime = runtime.inner().enter();
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
    listener.set_nonblocking(true)?;
    let addr = listener.local_addr()?;
    let route = format!("/media/{}/", uuid::Uuid::new_v4().simple());
    let scope = app.asset_protocol_scope();
    let listener = tokio::net::TcpListener::from_std(listener)?;
    let _ = ORIGINAL_MEDIA_BASE_URL.set(format!("http://{addr}{route}"));

    tauri::async_runtime::spawn(async move {
        let route: Arc<str> = route.into();
        while let Ok((stream, _)) = listener.accept().await {
            let scope = scope.clone();
            let route = route.clone();
            tokio::spawn(async move {
                let _ = handle_connection(stream, &route, move |path| scope.is_allowed(path)).await;
            });
        }
    });
    Ok(())
}

#[tauri::command]
pub fn original_media_base_url() -> Result<&'static str, String> {
    ORIGINAL_MEDIA_BASE_URL
        .get()
        .map(String::as_str)
        .ok_or_else(|| "Original media server not initialized".to_string())
}

pub fn make_video_stream_url(file_path: &str) -> Result<String, String> {
    let encoded =
        percent_encoding::utf8_percent_encode(file_path, percent_encoding::NON_ALPHANUMERIC);
    Ok(format!("{}{}", original_media_base_url()?, encoded))
}

fn parse_http_range(range_header: &str, file_size: u64) -> Option<(u64, u64)> {
    if file_size == 0 {
        return None;
    }
    let range = range_header
        .strip_prefix("bytes=")?
        .split(',')
        .next()?
        .trim();
    let (start_str, end_str) = range.split_once('-')?;

    if start_str.is_empty() {
        let suffix_len = end_str.parse::<u64>().ok()?;
        if suffix_len == 0 {
            return None;
        }
        let start = file_size.saturating_sub(suffix_len);
        let end = file_size.saturating_sub(1);
        return Some((start, end));
    }

    let start = start_str.parse::<u64>().ok()?;
    if start >= file_size {
        return None;
    }

    let end = if end_str.is_empty() {
        file_size.saturating_sub(1)
    } else {
        end_str
            .parse::<u64>()
            .ok()?
            .min(file_size.saturating_sub(1))
    };

    if start > end {
        return None;
    }

    Some((start, end))
}

async fn write_headers(
    stream: &mut TcpStream,
    status: &str,
    keep_alive: bool,
    headers: &[(&str, String)],
) -> io::Result<()> {
    let connection = if keep_alive { "keep-alive" } else { "close" };
    let mut response = format!(
        "HTTP/1.1 {status}\r\nConnection: {connection}\r\nReferrer-Policy: no-referrer\r\nContent-Security-Policy: sandbox; default-src 'none'\r\nX-Content-Type-Options: nosniff\r\n"
    );
    for (name, value) in headers {
        response.push_str(&format!("{name}: {value}\r\n"));
    }
    response.push_str("\r\n");
    stream.write_all(response.as_bytes()).await
}

async fn reject(stream: &mut TcpStream, status: &str, keep_alive: bool) -> io::Result<()> {
    write_headers(
        stream,
        status,
        keep_alive,
        &[("Content-Length", "0".into())],
    )
    .await
}

async fn read_request(stream: &mut TcpStream, pending: &mut Vec<u8>) -> io::Result<Option<String>> {
    const MAX_HEADER_BYTES: usize = 16 * 1024;
    let mut chunk = [0u8; 2048];
    loop {
        if let Some(end) = pending.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            if end + 4 > MAX_HEADER_BYTES {
                return Ok(None);
            }
            let request = std::str::from_utf8(&pending[..end]).ok().map(str::to_owned);
            pending.drain(..end + 4);
            return Ok(request);
        }
        if pending.len() >= MAX_HEADER_BYTES {
            return Ok(None);
        }
        let count = stream.read(&mut chunk).await?;
        if count == 0 {
            return Ok(None);
        }
        pending.extend_from_slice(&chunk[..count]);
    }
}

struct Request<'a> {
    method: &'a str,
    target: &'a str,
    origin: Option<&'a str>,
    range: Option<&'a str>,
    if_none_match: Vec<&'a str>,
    keep_alive: bool,
}

impl<'a> Request<'a> {
    fn parse(head: &'a str) -> Option<Self> {
        let mut lines = head.split("\r\n");
        let mut parts = lines.next()?.split(' ');
        let method = parts.next().filter(|part| !part.is_empty())?;
        let target = parts.next().filter(|part| part.starts_with('/'))?;
        let version = parts.next()?;
        if !matches!(version, "HTTP/1.0" | "HTTP/1.1") || parts.next().is_some() {
            return None;
        }

        let mut origin = None;
        let mut range = None;
        let mut if_none_match = Vec::new();
        let mut close = false;
        let mut keep_alive = version == "HTTP/1.1";
        let mut has_content_length = false;
        for line in lines {
            let (name, value) = line.split_once(':')?;
            let name = http::HeaderName::from_bytes(name.as_bytes()).ok()?;
            http::HeaderValue::from_str(value).ok()?;
            let value = value.trim();
            match name.as_str() {
                "origin" => {
                    if origin.replace(value).is_some() {
                        return None;
                    }
                }
                "range" if range.is_none() => range = Some(value),
                "if-none-match" => if_none_match.push(value),
                "connection" => {
                    for option in value.split(',').map(str::trim) {
                        close |= option.eq_ignore_ascii_case("close");
                        keep_alive |= option.eq_ignore_ascii_case("keep-alive");
                    }
                }
                // The media endpoint accepts only requests without bodies. Leaving
                // unread body bytes on a persistent socket would desync framing.
                "transfer-encoding" => return None,
                "content-length" => {
                    if has_content_length
                        || value.is_empty()
                        || !value.bytes().all(|byte| byte == b'0')
                    {
                        return None;
                    }
                    has_content_length = true;
                }
                _ => {}
            }
        }
        Some(Self {
            method,
            target,
            origin,
            range,
            if_none_match,
            keep_alive: keep_alive && !close,
        })
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    route: &str,
    is_allowed: impl Fn(&Path) -> bool + Send + Sync + 'static,
) -> io::Result<()> {
    let is_allowed = Arc::new(is_allowed);
    let mut pending = Vec::with_capacity(2048);
    while let Ok(request) = tokio::time::timeout(
        Duration::from_secs(5),
        read_request(&mut stream, &mut pending),
    )
    .await
    {
        let Some(head) = request? else {
            break;
        };
        let Some(request) = Request::parse(&head) else {
            write_headers(
                &mut stream,
                "400 Bad Request",
                false,
                &[("Content-Length", "0".into())],
            )
            .await?;
            break;
        };
        let keep_alive = request.keep_alive;
        serve_request(&mut stream, route, is_allowed.clone(), request).await?;
        if !keep_alive {
            break;
        }
    }
    Ok(())
}

async fn serve_request(
    stream: &mut TcpStream,
    route: &str,
    is_allowed: Arc<impl Fn(&Path) -> bool + Send + Sync + 'static>,
    request: Request<'_>,
) -> io::Result<()> {
    let Request {
        method,
        target,
        origin,
        range,
        if_none_match,
        keep_alive,
    } = request;
    let target = target.split_once('?').map_or(target, |(path, _)| path);
    let Some(encoded) = target.strip_prefix(route) else {
        return reject(stream, "404 Not Found", keep_alive).await;
    };
    let mut cors = vec![("Vary", "Origin".into())];
    if let Some(origin) = origin {
        // WKWebView can serialize a custom-scheme origin as opaque.
        // Every request still requires the per-process route token.
        let trusted = matches!(
            origin,
            "tauri://localhost" | "http://tauri.localhost" | "https://tauri.localhost" | "null"
        ) || (cfg!(debug_assertions) && origin == "http://localhost:3580");
        if !trusted {
            return reject(stream, "403 Forbidden", keep_alive).await;
        }
        cors.push(("Access-Control-Allow-Origin", origin.into()));
    }
    if method == "OPTIONS" {
        cors.extend([
            ("Access-Control-Allow-Methods", "GET, HEAD, OPTIONS".into()),
            ("Access-Control-Allow-Headers", "Range".into()),
        ]);
        return write_headers(stream, "204 No Content", keep_alive, &cors).await;
    }
    if method != "GET" && method != "HEAD" {
        return reject(stream, "405 Method Not Allowed", keep_alive).await;
    }
    let Ok(path) = percent_encoding::percent_decode_str(encoded).decode_utf8() else {
        return reject(stream, "400 Bad Request", keep_alive).await;
    };
    let Ok(path) = tauri::path::SafePathBuf::new(std::path::PathBuf::from(path.as_ref())) else {
        return reject(stream, "403 Forbidden", keep_alive).await;
    };
    let opened = tokio::task::spawn_blocking(move || {
        let path = path.as_ref();
        if !path.is_absolute() || !is_allowed(path) {
            return Err("403 Forbidden");
        }
        let metadata = std::fs::metadata(path).map_err(|_| "404 Not Found")?;
        if !metadata.is_file() || crate::t_utils::is_cloud_only(&metadata) {
            return Err("403 Forbidden");
        }
        let file = std::fs::File::open(path).map_err(|_| "404 Not Found")?;
        Ok((
            tokio::fs::File::from_std(file),
            path.to_string_lossy().into_owned(),
        ))
    })
    .await;
    let (mut file, path) = match opened {
        Ok(Ok(opened)) => opened,
        Ok(Err(status)) => return reject(stream, status, keep_alive).await,
        Err(_) => return reject(stream, "500 Internal Server Error", keep_alive).await,
    };
    let metadata = file.metadata().await?;
    let file_size = metadata.len();
    let cacheable = method == "GET" && range.is_none();
    let cache_control = if cacheable {
        "private, no-cache"
    } else {
        "no-store"
    };
    let mut headers = cors;
    headers.extend([
        (
            "Access-Control-Expose-Headers",
            "Content-Range, Accept-Ranges".into(),
        ),
        ("Accept-Ranges", "bytes".into()),
        ("Cache-Control", cache_control.into()),
    ]);
    if cacheable {
        let etag = metadata.modified().ok().map(|modified| {
            let nanos = match modified.duration_since(UNIX_EPOCH) {
                Ok(duration) => duration.as_nanos() as i128,
                Err(error) => -(error.duration().as_nanos() as i128),
            };
            format!("\"{file_size:x}-{nanos}\"")
        });
        let not_modified = if_none_match.iter().any(|value| {
            *value == "*"
                || etag.as_ref().is_some_and(|etag| {
                    value
                        .split(',')
                        .map(str::trim)
                        .any(|tag| tag.strip_prefix("W/").unwrap_or(tag) == etag)
                })
        });
        if let Some(etag) = etag {
            headers.push(("ETag", etag));
        }
        if not_modified {
            return write_headers(stream, "304 Not Modified", keep_alive, &headers).await;
        }
    }
    let (status, offset, length) = if let Some(range) = range {
        let Some((start, end)) = parse_http_range(range, file_size) else {
            headers.push(("Content-Range", format!("bytes */{file_size}")));
            headers.push(("Content-Length", "0".into()));
            return write_headers(stream, "416 Range Not Satisfiable", keep_alive, &headers).await;
        };
        headers.push(("Content-Range", format!("bytes {start}-{end}/{file_size}")));
        ("206 Partial Content", start, end - start + 1)
    } else {
        ("200 OK", 0, file_size)
    };

    let content_type = match std::path::Path::new(&path)
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "jxl" => "image/jxl".into(),
        "mp4" | "m4v" => "video/mp4".into(),
        "mov" => "video/quicktime".into(),
        "webm" => "video/webm".into(),
        "ogv" => "video/ogg".into(),
        _ => {
            let mut prefix = Vec::with_capacity(8192);
            (&mut file).take(8192).read_to_end(&mut prefix).await?;
            tauri_utils::mime_type::MimeType::parse(&prefix, &path)
        }
    };
    headers.push(("Content-Type", content_type));
    headers.push(("Content-Length", length.to_string()));
    if method != "HEAD" {
        file.seek(SeekFrom::Start(offset)).await?;
    }
    write_headers(stream, status, keep_alive, &headers).await?;
    if method != "HEAD" && length > 0 {
        let capacity = length.min(128 * 1024) as usize;
        let mut body = tokio::io::BufReader::with_capacity(capacity, file.take(length));
        if tokio::io::copy_buf(&mut body, stream).await? != length {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "media changed during response",
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader};

    #[tokio::test]
    async fn reuses_a_socket_and_preserves_pipelined_request_boundaries() {
        tokio::time::timeout(Duration::from_secs(3), async {
            let path = std::env::temp_dir().join(format!("lap-http-{}.mp4", uuid::Uuid::new_v4()));
            std::fs::write(&path, b"0123456789").unwrap();
            let set_mtime = |nanos| {
                std::fs::File::options().write(true).open(&path).unwrap()
                    .set_times(std::fs::FileTimes::new().set_modified(
                        UNIX_EPOCH + Duration::new(1_700_000_000, nanos),
                    ))
                    .unwrap();
            };
            set_mtime(100_000_000);
            let target = format!("/media/test/{}?u=1", percent_encoding::utf8_percent_encode(
                path.to_str().unwrap(), percent_encoding::NON_ALPHANUMERIC,
            ));
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
            let mut client = BufReader::new(TcpStream::connect(listener.local_addr().unwrap()).await.unwrap());
            let (server, _) = listener.accept().await.unwrap();
            let allowed = path.clone();
            let serve = tokio::spawn(handle_connection(server, "/media/test/", move |path| path == allowed));

            async fn read_headers(client: &mut BufReader<TcpStream>) -> String {
                let mut headers = String::new();
                loop {
                    let start = headers.len();
                    assert_ne!(client.read_line(&mut headers).await.unwrap(), 0);
                    if &headers[start..] == "\r\n" {
                        return headers;
                    }
                }
            }

            client.get_mut().write_all(format!("GET {target} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes()).await.unwrap();
            let headers = read_headers(&mut client).await;
            assert!(headers.contains("Connection: keep-alive"));
            assert!(headers.contains("Cache-Control: private, no-cache"));
            assert!(headers.contains("Content-Length: 10"));
            let etag = headers.lines().find_map(|line| line.strip_prefix("ETag: ")).unwrap();
            assert!(etag.starts_with('"') && etag.ends_with('"'));
            let mut body = [0; 10];
            client.read_exact(&mut body).await.unwrap();
            assert_eq!(&body, b"0123456789");

            client.get_mut().write_all(format!(
                "GET {target} HTTP/1.1\r\nHost: localhost\r\nIf-None-Match: \"other\", W/{etag}\r\n\r\nHEAD {target} HTTP/1.1\r\nHost: localhost\r\n\r\n"
            ).as_bytes()).await.unwrap();
            let conditional = read_headers(&mut client).await;
            assert!(conditional.starts_with("HTTP/1.1 304"));
            assert!(conditional.contains(&format!("ETag: {etag}\r\n")));
            assert!(conditional.contains("Cache-Control: private, no-cache"));
            let headers = read_headers(&mut client).await;
            assert!(headers.starts_with("HTTP/1.1 200"));
            assert!(headers.contains("Content-Length: 10"));

            std::fs::write(&path, b"abcdefghij").unwrap();
            set_mtime(200_000_000);
            client.get_mut().write_all(format!(
                "GET {target} HTTP/1.1\r\nHost: localhost\r\nIf-None-Match: {etag}\r\n\r\n"
            ).as_bytes()).await.unwrap();
            let headers = read_headers(&mut client).await;
            assert!(headers.starts_with("HTTP/1.1 200"));
            let new_etag = headers.lines().find_map(|line| line.strip_prefix("ETag: ")).unwrap();
            assert_ne!(new_etag, etag);
            client.read_exact(&mut body).await.unwrap();
            assert_eq!(&body, b"abcdefghij");

            client.get_mut().write_all(format!(
                "GET {target} HTTP/1.1\r\nHost: localhost\r\nIf-None-Match: {new_etag}\r\nRange: bytes=2-4\r\nConnection: close\r\n\r\n"
            ).as_bytes()).await.unwrap();
            let headers = read_headers(&mut client).await;
            assert!(headers.starts_with("HTTP/1.1 206"));
            assert!(headers.contains("Connection: close"));
            assert!(headers.contains("Cache-Control: no-store"));
            let mut body = Vec::new();
            client.read_to_end(&mut body).await.unwrap();
            assert_eq!(body, b"cde");
            serve.await.unwrap().unwrap();
            std::fs::remove_file(path).unwrap();
        }).await.unwrap();
    }

    #[test]
    fn rejects_framing_that_would_leave_request_body_bytes_unread() {
        let request = "GET /media/test/file HTTP/1.1\r\nHost: localhost";
        assert!(Request::parse(&format!("{request}\r\nTransfer-Encoding: chunked")).is_none());
        assert!(Request::parse(&format!("{request}\r\nContent-Length: 1")).is_none());
        assert!(Request::parse(&format!("{request}\r\nContent-Length: 0")).is_some());
    }

    #[tokio::test]
    async fn streams_full_files_and_ranges_only_with_the_route_and_scope() {
        let path = std::env::temp_dir().join(format!("Lap {} # 示例.mp4", uuid::Uuid::new_v4()));
        std::fs::write(&path, b"0123456789").unwrap();
        let encoded = percent_encoding::utf8_percent_encode(
            path.to_str().unwrap(),
            percent_encoding::NON_ALPHANUMERIC,
        );
        let target = format!("/media/test/{encoded}?v=1");

        async fn request(
            target: &str,
            method: &str,
            range: &str,
            permitted: Option<std::path::PathBuf>,
        ) -> (String, Vec<u8>) {
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
                .await
                .unwrap();
            let mut client = TcpStream::connect(listener.local_addr().unwrap())
                .await
                .unwrap();
            let (server, _) = listener.accept().await.unwrap();
            let serve = tokio::spawn(handle_connection(server, "/media/test/", move |path| {
                permitted.as_deref() == Some(path)
            }));
            client
                .write_all(format!("{method} {target} HTTP/1.1\r\n").as_bytes())
                .await
                .unwrap();
            client
                .write_all(
                    format!("Host: localhost\r\nConnection: close\r\n{range}\r\n").as_bytes(),
                )
                .await
                .unwrap();
            let mut response = Vec::new();
            client.read_to_end(&mut response).await.unwrap();
            serve.await.unwrap().unwrap();
            let separator = response
                .windows(4)
                .position(|chunk| chunk == b"\r\n\r\n")
                .unwrap();
            (
                String::from_utf8(response[..separator].to_vec()).unwrap(),
                response[separator + 4..].to_vec(),
            )
        }

        let (headers, body) = request(&target, "GET", "", Some(path.clone())).await;
        assert!(headers.starts_with("HTTP/1.1 200"));
        assert!(headers.contains("Content-Type: video/mp4"));
        assert!(headers.contains("Cache-Control: private, no-cache"));
        assert_eq!(body, b"0123456789");

        let (headers, body) =
            request(&target, "GET", "Range: bytes=2-4\r\n", Some(path.clone())).await;
        assert!(headers.starts_with("HTTP/1.1 206"));
        assert!(headers.contains("Content-Range: bytes 2-4/10"));
        assert!(headers.contains("Cache-Control: no-store"));
        assert_eq!(body, b"234");

        let (headers, body) =
            request(&target, "HEAD", "Range: bytes=-2\r\n", Some(path.clone())).await;
        assert!(headers.starts_with("HTTP/1.1 206"));
        assert!(headers.contains("Content-Range: bytes 8-9/10"));
        assert!(body.is_empty());

        let (headers, _) =
            request(&target, "GET", "Range: bytes=10-\r\n", Some(path.clone())).await;
        assert!(headers.starts_with("HTTP/1.1 416"));

        let (headers, _) = request(&target, "GET", "", None).await;
        assert!(headers.starts_with("HTTP/1.1 403"));
        let (headers, _) = request("/media/incorrect/file", "GET", "", Some(path.clone())).await;
        assert!(headers.starts_with("HTTP/1.1 404"));

        let (headers, body) =
            request(&target, "OPTIONS", "Origin: tauri://localhost\r\n", None).await;
        assert!(headers.starts_with("HTTP/1.1 204"));
        assert!(headers.contains("Access-Control-Allow-Origin: tauri://localhost"));
        assert!(body.is_empty());

        let (headers, _) = request(
            &target,
            "GET",
            "Origin: https://example.com\r\n",
            Some(path.clone()),
        )
        .await;
        assert!(headers.starts_with("HTTP/1.1 403"));

        let (headers, body) = request(&target, "HEAD", "", Some(path.clone())).await;
        assert!(headers.contains("Content-Length: 10"));
        assert!(body.is_empty());

        let (headers, _) = request(
            "/media/test/relative.mp4",
            "GET",
            "",
            Some("relative.mp4".into()),
        )
        .await;
        assert!(headers.starts_with("HTTP/1.1 403"));

        // Original JXL bytes must be served without a JPEG preview conversion.
        let jxl_path = path.with_extension("jxl");
        let jxl_bytes = b"\xff\x0aJXL test payload";
        std::fs::write(&jxl_path, jxl_bytes).unwrap();
        let jxl_encoded = percent_encoding::utf8_percent_encode(
            jxl_path.to_str().unwrap(),
            percent_encoding::NON_ALPHANUMERIC,
        );
        let jxl_target = format!("/media/test/{jxl_encoded}?v=1");
        let (headers, body) = request(&jxl_target, "GET", "", Some(jxl_path.clone())).await;
        assert!(headers.starts_with("HTTP/1.1 200"));
        assert!(headers.contains("Content-Type: image/jxl"));
        assert_eq!(body, jxl_bytes);
        std::fs::remove_file(jxl_path).unwrap();

        std::fs::write(&path, b"").unwrap();
        let (headers, body) = request(&target, "GET", "", Some(path.clone())).await;
        assert!(headers.contains("Content-Length: 0"));
        assert!(body.is_empty());
        let (headers, _) = request(&target, "GET", "Range: bytes=0-\r\n", Some(path.clone())).await;
        assert!(headers.starts_with("HTTP/1.1 416"));

        let image_path = path.with_extension("png");
        let image = b"\x89PNG\r\n\x1a\n";
        std::fs::write(&image_path, image).unwrap();
        let encoded = percent_encoding::utf8_percent_encode(
            image_path.to_str().unwrap(),
            percent_encoding::NON_ALPHANUMERIC,
        );
        let (headers, body) = request(
            &format!("/media/test/{encoded}"),
            "GET",
            "",
            Some(image_path.clone()),
        )
        .await;
        assert!(headers.contains("Content-Type: image/png"));
        assert!(headers.contains("Cache-Control: private, no-cache"));
        assert!(headers.contains("ETag: \""));
        assert_eq!(body, image);

        std::fs::remove_file(image_path).unwrap();
        std::fs::remove_file(path).unwrap();
    }
}
