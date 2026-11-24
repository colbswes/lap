// The macOS menu owns the Cmd-A accelerator, so the keystroke never reaches a
// focused text field. The menu event lands here instead and must select the
// field's text rather than the photo grid.
const NON_TEXT_INPUT_TYPES = new Set([
  'button', 'checkbox', 'color', 'file', 'hidden', 'image', 'radio', 'range', 'reset', 'submit',
]);

export function isTextEditable(el: Element | null | undefined): boolean {
  if (!el) return false;
  const html = el as HTMLElement;
  if (html.isContentEditable) return true;
  if (el.tagName === 'TEXTAREA') return true;
  if (el.tagName === 'INPUT') {
    return !NON_TEXT_INPUT_TYPES.has(((el as HTMLInputElement).type || 'text').toLowerCase());
  }
  return false;
}

/** Selects the focused field's text. Returns true when a text field had focus. */
export function selectAllInFocusedTextField(doc: Document = document): boolean {
  const active = doc.activeElement;
  if (!isTextEditable(active)) return false;
  if (active!.tagName === 'INPUT' || active!.tagName === 'TEXTAREA') {
    (active as HTMLInputElement | HTMLTextAreaElement).select();
  } else {
    doc.execCommand('selectAll');
  }
  return true;
}
