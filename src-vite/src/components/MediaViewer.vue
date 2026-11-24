<template>
  <Teleport to="body" :disabled="!previewFullScreen">
  <div 
    :class="['w-full flex flex-col items-center justify-center', previewFullScreen ? 'fixed inset-0 z-[200]' : 'relative', isFullScreen ? 'bg-base-200' : '', toolbarOnly ? '' : 'h-full group']"
    :style="toolbarOnly ? undefined : viewBackgroundStyle"
    @mousemove="handleMouseMove"
    @mouseover="handleRawControlsHover"
    @mouseleave="handleMouseLeave"
    @contextmenu.prevent
    ref="containerRef"
  >
    <!-- Toolbar -->
    <div 
      v-if="showToolbar"
      id="responsiveDiv"
      :class="computedToolbarClass"
      data-tauri-drag-region
    >
      <!-- App Icon + Title (left side, ImageViewer on Windows) -->
      <div v-if="showDesktopWindowControls && mode === 2 && showWindowControlsBar" class="absolute left-0 top-0 h-10 flex items-center px-3 select-none" data-tauri-drag-region>
        <img :src="iconLogo" class="w-5 h-5 mr-2 rounded" data-tauri-drag-region />
        <span class="text-nowrap text-sm text-base-content/70 overflow-hidden whitespace-pre text-ellipsis" data-tauri-drag-region>
          {{ $t('image_viewer.title') }}
        </span>
      </div>
      <div ref="buttonsRef" class="flex items-center space-x-1">
        <TButton
          :icon="IconPrev"
          :disabled="fileIndex <= 0 || isSlideShow || !canInteract"
          :tooltip="$t('image_viewer.toolbar.prev')"
          @click="triggerPrev" 
        />
        <TButton
          :icon="IconNext"
          :disabled="fileIndex < 0 || fileIndex >= fileCount - 1 || isSlideShow || !canInteract"
          :tooltip="$t('image_viewer.toolbar.next')"
          @click="triggerNext" 
        />
        <div class="inline-flex shrink-0 items-center gap-0 overflow-hidden rounded-box border transition-colors duration-200"
          :class="isSlideShow && canSlideShow ? 'border-base-content/5' : 'border-transparent'"
          role="group" :aria-label="$t('image_viewer.toolbar.slide_show')">
          <TButton
            :icon="isSlideShow ? IconPause : IconPlay"
            buttonClasses="h-7! w-7! min-h-0!"
            :disabled="fileIndex < 0 || !canSlideShow || !canInteract"
            :selected="isSlideShow && canSlideShow"
            :tooltip="!canSlideShow
              ? $t('image_viewer.toolbar.slide_show')
              : (isSlideShow ? $t('image_viewer.toolbar.pause') : $t('image_viewer.toolbar.slide_show'))"
            @click="handleToggleSlideShow"
          />
          <Transition name="play-interval">
          <div v-if="isSlideShow && canSlideShow" class="play-interval-slot shrink-0">
          <ContextMenu
            :menuItems="slideShowIntervalMenuItems"
            :disabled="fileIndex < 0 || !canSlideShow || !canInteract"
            @open-change="handleMenuOpenChange"
            @click.stop
          >
            <template #trigger="{ toggle }">
              <button
                class="h-7 w-14 px-1 inline-flex items-center justify-center gap-0.5 rounded-box text-xs cursor-pointer"
                :class="[
                  fileIndex < 0 || !canSlideShow || !canInteract
                    ? 'cursor-default text-base-content/30'
                    : 'text-base-content/70 hover:bg-base-100/30'
                ]"
                :disabled="fileIndex < 0 || !canSlideShow || !canInteract"
                :title="$t('settings.image_view.slide_show_interval', { second: getSlideShowInterval(effectiveSlideShowIntervalIndex) })"
                @click.stop="toggle"
              >
                <span>{{ currentSlideShowIntervalLabel }}</span>
                <IconArrowDown class="w-3 h-3" />
              </button>
            </template>
          </ContextMenu>
          </div>
          </Transition>
        </div>
        <div class="mx-1 inline-flex shrink-0 items-center gap-0 overflow-hidden rounded-box border border-base-content/5" role="group" :aria-label="$t('image_viewer.toolbar.zoom_fit')">
          <TButton
            :icon="IconZoomOut"
            buttonClasses="h-7! w-7! min-h-0!"
            :disabled="fileIndex < 0 || imageScale <= imageMinScale || isSlideShow || !canInteract"
            :tooltip="$t('image_viewer.toolbar.zoom_out')"
            :shortcut="shortcut('view.zoomOut')"
            @click="handleZoomOut"
          />
          <ContextMenu :menuItems="zoomPercentMenuItems" :disabled="fileIndex < 0 || isSlideShow || !canInteract" @open-change="handleMenuOpenChange" @click.stop>
            <template #trigger="{ toggle }">
              <button class="h-7 w-14 rounded-box inline-flex items-center justify-center gap-0.5 text-xs tabular-nums text-base-content/70 hover:bg-base-100/30 cursor-pointer"
                :disabled="fileIndex < 0 || isSlideShow || !canInteract" @click.stop="toggle">
                {{ zoomScaleLabel }}<IconArrowDown class="size-3" />
              </button>
            </template>
          </ContextMenu>
          <TButton
            :icon="IconZoomIn"
            buttonClasses="h-7! w-7! min-h-0!"
            :disabled="fileIndex < 0 || imageScale >= imageMaxScale || isSlideShow || !canInteract"
            :tooltip="$t('image_viewer.toolbar.zoom_in')"
            :shortcut="shortcut('view.zoomIn')"
            @click="handleZoomIn"
          />
        </div>
        <TButton
          :icon="IconRotate"
          :disabled="fileIndex < 0 || isSlideShow || !canInteract"
          :iconStyle="{ transform: `rotate(${file?.rotate ?? 0}deg)`, transition: 'transform 0.3s' }"
          :selected="(file?.rotate ?? 0) % 360 !== 0 && !isSlideShow"
          :tooltip="$t('menu.meta.rotate')"
          :shortcut="shortcut('meta.rotate')"
          @click="$emit('item-action', { action: 'rotate', index: fileIndex })"
        />
        <TButton
          :icon="IconAutoFocus"
          :selected="showAfAreas"
          :disabled="fileIndex < 0 || isSlideShow || !canInteract || ![1, 3].includes(Number(file?.file_type))"
          :tooltip="$t('image_viewer.toolbar.af_area')"
          :shortcut="shortcut('view.toggleAfAreas')"
          @click="toggleAfAreas"
        />
        <TButton
          v-if="config.settings.face.enabled"
          :icon="IconPerson"
          :selected="Boolean(config.mediaViewer.showFaces)"
          :disabled="fileIndex < 0 || isSlideShow || !canInteract || ![1, 2, 3].includes(Number(file?.file_type))"
          :tooltip="config.mediaViewer.showFaces ? $t('image_viewer.toolbar.hide_faces') : $t('image_viewer.toolbar.show_faces')"
          @click="config.mediaViewer.showFaces = !config.mediaViewer.showFaces"
        />
        <template v-if="showExtraIcons">
          <IconSeparator class="t-icon-size-sm text-base-content/30" />
          <TButton
            :icon="file?.is_favorite ? IconHeartFilled : IconHeart"
            :disabled="fileIndex < 0 || isSlideShow || !canInteract"
            :selected="file?.is_favorite && !isSlideShow"
            :tooltip="file?.is_favorite ? $t('menu.meta.unfavorite') : $t('menu.meta.favorite')"
            :shortcut="shortcut('meta.favorite')"
            @click="$emit('item-action', { action: 'favorite', index: fileIndex })"
          />
          <TButton
            :icon="IconTag"
            :disabled="fileIndex < 0 || isSlideShow || !canInteract"
            :selected="file?.has_tags && !isSlideShow"
            :tooltip="$t('menu.meta.tag')"
            :shortcut="shortcut('meta.tag')"
            @click="$emit('item-action', { action: 'tag', index: fileIndex })"
          />
          <TButton
            :icon="IconCollection"
            :disabled="fileIndex < 0 || isSlideShow || !canInteract"
            :selected="file?.has_collections && !isSlideShow"
            :tooltip="$t('menu.meta.collection')"
            :shortcut="shortcut('meta.collection')"
            @click="$emit('item-action', { action: 'add-to-collection', index: fileIndex })"
          />
          <!-- <TButton
            v-if="mode !== 2"
            :icon="IconFileInfo"
            :disabled="fileIndex < 0 || isSlideShow || !canInteract"
            :tooltip="$t('menu.file.show_info')"
            :shortcut="shortcut('meta.info')"
            @click="$emit('item-action', { action: 'info', index: fileIndex })"
          /> -->
        </template>
        <!-- Linked viewport control (Compare mode only) -->
        <template v-if="mode === 2 && showSyncViewportControl">
          <IconSeparator class="t-icon-size-sm text-base-content/30" />
          <TButton
            :icon="IconLink"
            :selected="isSyncViewport"
            :tooltip="isSyncViewport ? $t('image_viewer.toolbar.sync_viewport_off') : $t('image_viewer.toolbar.sync_viewport_on')"
            @click="$emit('item-action', { action: 'toggle-sync-viewport' })"
          />
        </template>
        <ContextMenu v-if="mode !== 2"
          ref="contextMenuRef"
          :iconMenu="IconMore"
          :tooltip="$t('info_panel.more')"
          :menuItems="singleFileMenuItems"
          :disabled="fileIndex < 0 || isSlideShow || !canInteract"
          @open-change="handleMenuOpenChange"
          @click.stop
        />
        <IconSeparator v-if="mode !== 2" class="t-icon-size-sm text-base-content/30" />
        <TButton
          v-if="mode === 2 && showViewerMenu"
          :icon="IconMore"
          :tooltip="$t('info_panel.more')"
          :disabled="fileIndex < 0 || isSlideShow || !canInteract"
          @click.stop="$emit('viewer-more', $event)"
        />
        <TButton
          :icon="!isFullScreen ? IconFullScreen : IconRestoreScreen"
          :tooltip="!isFullScreen ? $t('image_viewer.toolbar.fullscreen') : $t('image_viewer.toolbar.exit_fullscreen')"
          :shortcut="shortcut('view.toggleFullscreen')"
          @click="toggleFullScreen"
        />
        <TButton v-if="mode !== 2 && !isFullScreen"
          :icon="config.mediaViewer.isPinned ? IconPin : IconUnPin"
          :disabled="fileIndex < 0 || !canInteract"
          :tooltip="!config.mediaViewer.isPinned ? $t('image_viewer.toolbar.pin') : $t('image_viewer.toolbar.unpin')"
          @click="toggleToolbarPin"
        />
        <TButton
          v-if="isFullScreen || (mode === 0 && config.mediaViewer.isPinned)"
          :icon="IconClose"
          :tooltip="$t('image_viewer.toolbar.close')"
          @click.stop="$emit('close')"
        />
      </div>
    </div>

    <!-- Window Control Buttons (top-right) -->
    <div v-if="showWindowControlsBar && showWindowControls && showDesktopWindowControls" class="absolute top-0 right-0 z-90 flex items-center" @mousedown.stop>
      <IconWinMinus 
        class="p-3 w-12 h-10 text-base-content/70 hover:text-base-content hover:bg-base-100 transition-colors duration-300 cursor-pointer" 
        @click.stop="minimizeWindow" 
      />
      <component :is="isMaximized ? IconWinRestore : IconWinMaximize" 
        class="p-3 w-12 h-10 text-base-content/70 hover:text-base-content hover:bg-base-100 transition-colors duration-300 cursor-pointer" 
        @click.stop="toggleMaximizeWindow" 
      />
      <IconClose 
        class="p-3 w-12 h-10 text-base-content/70 hover:text-base-content hover:bg-red-500 transition-colors duration-300 cursor-pointer" 
        @click.stop="$emit('close')" 
      />
    </div>

    <!-- Elements below only rendered when not toolbar-only -->
    <template v-if="!toolbarOnly">
    <!-- Close Button (Top Right) -->
    <button 
      v-if="mode === 0 && !config.mediaViewer.isPinned && !isFullScreen"
      class="preview-press-button absolute right-2 top-2 z-90 p-2 rounded-full text-base-content/70 bg-base-100/30 hover:text-base-content hover:bg-base-100/70 cursor-pointer"
      @click.stop="$emit('close')"
      @dblclick.stop
    >
      <span class="preview-press-icon flex pointer-events-none"><IconClose class="w-5 h-5" /></span>
    </button>

    <div
      ref="mediaAreaRef"
      class="flex-1 w-full min-h-0 relative"
      @dblclick="$emit('media-dblclick')"
      @contextmenu.prevent="handleBackgroundContextMenu"
    >
      <Transition name="fade">
      <div
        v-if="showStatusBadges && (quickViewStatusBadges.length > 0 || statusFeedbackVisible)"
        class="pointer-events-none absolute left-4 top-4 z-30 flex max-w-[calc(100%-2.5rem)] flex-wrap gap-1"
      >
        <div v-if="unfavoriteFeedback && !file?.is_favorite" class="thumb-badge"
          role="status" :aria-label="$t('menu.meta.unfavorite')">
          <IconHeart class="h-3.5 w-3.5" />
        </div>
        <div v-else-if="statusFeedbackVisible && quickViewStatusBadges.length === 0" class="thumb-badge">—</div>
        <div
          v-for="badge in quickViewStatusBadges"
          :key="badge.key"
          class="thumb-badge"
        >
          <template v-if="badge.icons?.length">
            <div class="flex items-center gap-0.5">
              <component
                :is="entry.icon"
                v-for="(entry, index) in badge.icons"
                :key="`${badge.key}-${index}`"
                class="h-3.5 w-3.5 shrink-0"
              />
            </div>
          </template>
          <component
            v-else-if="badge.icon"
            :is="badge.icon"
            :class="['h-3.5 w-3.5 shrink-0', badge.iconClass]"
            :style="badge.iconStyle"
          />
          <span v-if="badge.label" class="leading-none">{{ badge.label }}</span>
        </div>
      </div>
      </Transition>

      <!-- Previous Button (Overlay, media-area anchored) -->
      <button
        v-if="!isSlideShow && (showOverlayNav || previewFullScreen)"
        class="preview-press-button absolute left-2 top-1/2 -translate-y-1/2 z-70 p-2 rounded-full bg-base-100/30 backdrop-blur-md transition-opacity duration-200"
        :class="[
          isHoverLeft ? (hasPrevious ? 'opacity-100 pointer-events-auto hover:text-base-content hover:bg-base-100/80 cursor-pointer' : 'opacity-30 cursor-default') : 'opacity-0 pointer-events-none'
        ]"
        :disabled="!hasPrevious"
        @click.stop="triggerPrev"
        @dblclick.stop
      >
        <span class="preview-press-icon flex pointer-events-none"><IconLeft class="w-8 h-8" /></span>
      </button>

      <!-- Next Button (Overlay, media-area anchored) -->
      <button
        v-if="!isSlideShow && (showOverlayNav || previewFullScreen)"
        class="preview-press-button absolute right-2 top-1/2 -translate-y-1/2 z-70 p-2 rounded-full bg-base-100/30 backdrop-blur-md transition-opacity duration-200"
        :class="[
          isHoverRight ? (hasNext ? 'opacity-100 pointer-events-auto hover:text-base-content hover:bg-base-100/80 cursor-pointer' : 'opacity-30 cursor-default') : 'opacity-0 pointer-events-none'
        ]"
        :disabled="!hasNext"
        @click.stop="triggerNext"
        @dblclick.stop
      >
        <span class="preview-press-icon flex pointer-events-none"><IconRight class="w-8 h-8" /></span>
      </button>

      <div
        v-if="isLivePhotoLike"
        :class="[
          'absolute inset-0 z-10 transition-opacity duration-150',
          isLivePhotoPlaying ? 'opacity-100' : 'pointer-events-none opacity-0',
        ]"
      >
        <Video
          class="h-full w-full"
          :filePath="livePhotoVideoPath"
          :rotate="file?.rotate ?? 0"
          :isZoomFit="isZoomFit"
          :isSlideShow="isSlideShow"
          :isActive="isPlaybackActive && isLivePhotoPlaying"
          :playOnActivate="true"
          :viewportState="livePhotoViewport"
          :showControls="false"
          :showPlayOverlay="false"
          @message-from-video-viewer="handleMessageFromImageViewer"
          @slideshow-next="emit('slideshow-next')"
          @context-menu="handleContextMenu"
          @pointerdown.capture="handleOverlayPointerDown"
          @pointerup.capture="handleOverlayPointerUp"
          @pointercancel.capture="resetOverlayPointer"
        ></Video>
      </div>

      <div
        v-if="hasImage"
        v-show="imageFile"
        :class="[
          isLivePhotoLike
            ? 'absolute inset-0 z-20 transition-opacity duration-150'
            : 'w-full h-full',
          isLivePhotoLike && (isLivePhotoPlaying ? 'pointer-events-none opacity-0' : 'opacity-100'),
        ]"
      >
        <Image
          ref="imageRef"
          :showAfAreas="showAfAreas"
          :afAreaFeedbackRequest="afAreaFeedbackRequest"
          :filePath="imageFile?.file_path"
          :fileId="imageFile?.id"
          :fileType="imageFile?.file_type"
          :originalUnavailable="originalUnavailable"
          :rawPairPath="imageFile?.media_subtype === 'raw_jpeg_pair' ? imageFile?.live_photo_video_path : ''"
          :fileVersion="imageFile?.modified_at || 0"
          :imageWidth="imageFile?.width"
          :imageHeight="imageFile?.height"
          :thumbnailSrc="imageFile?.thumbnail || ''"
          :showThumbnailPlaceholder="showThumbnailPlaceholder"
          :showInlineLoading="mode === 2"
          :suppressAutoNavigator="suppressAutoNavigator && !previewFullScreen"
          :nextFilePath="nextFilePath"
          :prefetchFiles="prefetchFiles"
          :loadDelay="loadDelay"
          :rotate="imageFile?.rotate ?? 0"
          :isZoomFit="isZoomFit"
          :isSlideShow="isSlideShow"
          :slideShowTransitionMode="slideShowTransitionMode"
          @update:isZoomFit="(val: boolean) => $emit('update:isZoomFit', val)"
          @scale="imageFile && handleScale($event)"
          @viewport-change="imageFile && handleViewportChange($event)"
          @message-from-image-viewer="handleMessageFromImageViewer"
          @context-menu="handleContextMenu"
          @pointerdown.capture="handleOverlayPointerDown"
          @pointerup.capture="handleOverlayPointerUp"
          @pointercancel.capture="resetOverlayPointer"
        ></Image>
      </div>

      <button
        v-if="isLivePhotoLike"
        class="absolute left-4 bottom-4 z-60 inline-flex h-10 items-center gap-2 rounded-box bg-base-100/70 px-3 text-sm font-medium text-base-content/70 shadow"
        :class="originalUnavailable ? 'cursor-default opacity-50' : 'hover:bg-base-100 hover:text-base-content cursor-pointer'"
        :disabled="originalUnavailable"
        @mouseenter="startLivePhotoPreview"
        @mouseleave="isLivePhotoPlaying = false"
        @click.stop
        @dblclick.stop
      >
        <IconLivePhoto class="h-4 w-4" />
        <span class="text-xs font-semibold uppercase tracking-wider leading-none">{{ isLivePhoto ? t('image_viewer.live') : t('image_viewer.motion') }}</span>
      </button>

      <div
        v-if="hasVideo"
        v-show="videoFile"
        class="absolute inset-0 z-20"
      >
        <img
          v-if="videoFile && !videoRef"
          :src="videoFile.thumbnail || getThumbUrl(videoFile.id, false, config.settings.thumbnailSize, videoFile.modified_at)"
          class="absolute inset-0 m-auto object-contain pointer-events-none"
          :class="isZoomFit ? 'w-full h-full' : 'max-w-full max-h-full'"
          decoding="sync"
          alt=""
        />
        <Video
          ref="videoRef"
          class="h-full w-full"
          :filePath="videoFile?.file_path"
          :fileId="videoFile?.id"
          :fileVersion="videoFile?.modified_at || 0"
          :loadDelay="loadDelay"
          :thumbnailSrc="videoFile?.thumbnail || ''"
          :originalUnavailable="originalUnavailable"
          :rotate="videoFile?.rotate ?? 0"
          :isZoomFit="isZoomFit"
          @update:isZoomFit="(val: boolean) => $emit('update:isZoomFit', val)"
          :isSlideShow="isSlideShow"
          :isActive="!!videoFile && isPlaybackActive"
          @scale="videoFile && handleScale($event)"
          @viewport-change="videoFile && handleViewportChange($event)"
          @message-from-video-viewer="handleMessageFromImageViewer"
          @slideshow-next="emit('slideshow-next')"
          @context-menu="handleContextMenu"
          @pointerdown.capture="handleOverlayPointerDown"
          @pointerup.capture="handleOverlayPointerUp"
          @pointercancel.capture="resetOverlayPointer"
        ></Video>

        <!-- Floating people/pets bar for videos when Show Faces is active -->
        <div
          v-if="showVideoFaceBar"
          data-video-face-toolbar
          class="absolute left-1/2 -translate-x-1/2 top-4 z-60 inline-flex max-w-[calc(100%-2rem)] flex-wrap items-center justify-center gap-1 rounded-box bg-base-100/75 backdrop-blur-md px-1.5 py-1 shadow border border-base-content/10 text-xs"
          @mousedown.stop
          @mouseup.stop
          @click.stop
          @dblclick.stop
        >
          <button
            v-for="face in videoFaces"
            :key="face.id"
            type="button"
            class="inline-flex h-7 items-center gap-1.5 rounded-box px-2 text-base-content/85 hover:bg-base-content/10 hover:text-base-content transition-colors cursor-pointer whitespace-nowrap"
            :title="$t(face.kind === 'pet' ? 'menu.person.edit_pet' : 'menu.person.edit_face')"
            @click="openVideoAssignFace(face)"
          >
            <span class="w-4 h-4 rounded-full overflow-hidden bg-base-300/70 ring-1 ring-base-content/10 shrink-0 flex items-center justify-center">
              <img
                v-if="face.thumbnail"
                :src="'data:image/jpeg;base64,' + face.thumbnail"
                class="w-full h-full object-cover"
                draggable="false"
              />
              <IconPerson v-else class="w-3 h-3 text-base-content/40" />
            </span>
            <span>{{ face.person_id ? (face.person_name || face.person_id) : $t(face.kind === 'pet' ? 'menu.person.unknown_pet' : 'menu.person.unknown_face') }}</span>
          </button>
          <span v-if="videoFaces.length > 0" class="h-4 w-px bg-base-content/15"></span>
          <button
            type="button"
            class="inline-flex h-7 items-center gap-1.5 rounded-box px-2.5 text-base-content/80 hover:bg-base-content/10 hover:text-base-content transition-colors cursor-pointer whitespace-nowrap"
            :disabled="isScanningVideoFaces"
            @click="openAddVideoManualFace"
          >
            <IconAdd class="size-3.5" />
            <span>{{ $t('menu.person.add_person_or_pet') }}</span>
          </button>
          <span class="h-4 w-px bg-base-content/15"></span>
          <button
            type="button"
            class="inline-flex h-7 items-center gap-1.5 rounded-box px-2.5 text-base-content/80 hover:bg-base-content/10 hover:text-base-content transition-colors cursor-pointer whitespace-nowrap disabled:opacity-50"
            :disabled="isScanningVideoFaces"
            :title="$t('menu.person.scan_video_tooltip')"
            @click="handleScanCurrentVideo"
          >
            <span v-if="isScanningVideoFaces" class="loading loading-spinner loading-xs"></span>
            <IconRefresh v-else class="size-3.5" />
            <span>{{ isScanningVideoFaces ? $t('menu.person.scan_photo_scanning') : $t('menu.person.scan_video') }}</span>
          </button>
        </div>
      </div>
    </div>

    <AssignFaceDialog
      v-if="videoAssignFace"
      :face="videoAssignFace"
      @cancel="videoAssignFace = null"
      @changed="onVideoFaceChanged"
    />

    </template>

    <ContextMenu
      ref="backgroundContextMenuRef"
      class="absolute h-0 w-0 overflow-hidden pointer-events-none"
      :menuItems="viewBackgroundMenuItems"
      @open-change="handleMenuOpenChange"
    >
      <template #trigger><span /></template>
    </ContextMenu>
  </div>
  </Teleport>
</template>

<script setup lang="ts">
import { OVERLAY_CLICK_MOVE_TOLERANCE, SIDEBAR } from '@/common/constants';
import { defineAsyncComponent, ref, computed, watch, onMounted, onUpdated, onBeforeUnmount, type Component, type CSSProperties, type PropType } from 'vue';
import { useI18n } from 'vue-i18n';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { runPreviewWindowOperation, recoverPreviewWindowFocus } from '@/common/previewWindow';
import { config, libConfig } from '@/common/config';
import { useToast } from '@/common/toast';
import { isWin, isMac, isLinux, getSlideShowInterval, getThumbUrl } from '@/common/utils';
import { getShortcutLabel, ShortcutActionId, ShortcutPlatform, VIEW_BACKGROUND_SHORTCUTS } from '@/common/shortcuts';
import { isOriginalUnavailable, setFileAccessibility } from '@/common/availability';
import { getMotionPhotoVideoPath, checkFileAccessibility, getFacesForFile, getFaceThumbnail, getPersonThumbnail, scanFileFaces } from '@/common/api';
import { PERSONS_CHANGED_EVENT } from '@/common/personContext';

import Image from '@/components/Image.vue';
import AssignFaceDialog, { type AssignFaceTarget } from '@/components/AssignFaceDialog.vue';
import { emit as tauriEmit, listen } from '@tauri-apps/api/event';
import TButton from '@/components/TButton.vue';
import { 
  IconLeft, 
  IconRight,
  IconPrev,
  IconNext,
  IconPlay,
  IconPause,
  IconArrowDown,
  IconZoomIn,
  IconZoomOut,
  IconFullScreen,
  IconRestoreScreen,
  IconPin,
  IconUnPin,
  IconSeparator,
  IconClose,
  IconMore,
  IconHeart,
  IconHeartFilled,
  IconCollection,
  IconTag,
  IconComment,
  IconRotate,
  IconFileInfo,
  IconDot,
  IconWinMinus,
  IconWinMaximize,
  IconWinRestore,
  IconLink,
  IconPalette,
  IconVideoPlay,
  IconLivePhoto,
  IconExclamation,
  IconAutoFocus,
  IconPerson,
  IconAdd,
  IconRefresh,
} from '@/common/icons';
import ContextMenu from '@/components/ContextMenu.vue';
import iconLogo from '@/assets/images/icon.png';
import { useFileMenuItems } from '@/common/fileMenu';
import type { PrefetchFile } from '@/common/viewerImages';

const Video = defineAsyncComponent(() => import('@/components/Video.vue'));

const props = defineProps({
  // 0: quick view, 1: filmstrip, 2: image viewer
  mode: {
    type: Number,
    default: 0
  },
  isFullScreen: {
    type: Boolean,
    default: false
  },
  file: {
    type: Object,
    default: null
  },
  hasPrevious: {
    type: Boolean,
    default: false
  },
  hasNext: {
    type: Boolean,
    default: false
  },
  fileIndex: {
    type: Number,
    default: -1
  },
  fileCount: {
    type: Number,
    default: 0
  },
  prefetchFiles: { type: Array as PropType<PrefetchFile[]>, default: () => [] },
  nextFilePath: {
    type: String,
    default: ''
  },
  suppressAutoNavigator: { type: Boolean, default: false },
  showThumbnailPlaceholder: {
    type: Boolean,
    default: false,
  },
  isSlideShow: {
    type: Boolean,
    default: false
  },
  canSlideShow: {
    type: Boolean,
    default: true
  },
  slideShowIntervalIndex: {
    type: Number,
    default: null
  },
  canInteract: {
    type: Boolean,
    default: true
  },
  displayZoomFit: { type: Boolean, default: null },
  imageScale: {
    type: Number,
    default: 1
  },
  imageMinScale: {
    type: Number,
    default: 0
  },
  imageMaxScale: {
    type: Number,
    default: 10
  },
  isZoomFit: {
    type: Boolean,
    default: true
  },
  showSyncViewportControl: {
    type: Boolean,
    default: false,
  },
  isPlaybackActive: {
    type: Boolean,
    default: true
  },
  isSyncViewport: {
    type: Boolean,
    default: false
  },
  showWindowControls: {
    type: Boolean,
    default: false
  },
  showToolbar: {
    type: Boolean,
    default: true
  },
  showOverlayNav: {
    type: Boolean,
    default: true
  },
  showViewerMenu: {
    type: Boolean,
    default: false
  },
  toolbarOnly: {
    type: Boolean,
    default: false
  },
  forceToolbarVisible: {
    type: Boolean,
    default: false
  },
});

const isLivePhotoPlaying = ref(false);
const isLivePhoto = computed(() => props.file?.media_subtype === 'live_photo' && !!props.file?.live_photo_video_path);
const isMotionPhoto = computed(() => props.file?.media_subtype === 'motion_photo');
// Both Apple Live Photos and Android Motion Photos play a short video layered
// over the still image, so they share the same UI/playback path.
const isLivePhotoLike = computed(() => isLivePhoto.value || isMotionPhoto.value);
const motionPhotoVideoPath = ref<string | null>(null);
const livePhotoVideoPath = computed(() =>
  isLivePhoto.value ? props.file?.live_photo_video_path : motionPhotoVideoPath.value,
);
const livePhotoViewport = ref<Record<string, number | boolean> | null>(null);
let motionPhotoVideoRequestSeq = 0;

function startLivePhotoPreview() {
  if (originalUnavailable.value || !livePhotoVideoPath.value) return;
  emit('activate');
  const viewport = mediaRef.value?.getViewportState?.();
  // Preserve the still image's visible region for the Live Photo preview.
  // Video.applyViewportState() remaps that normalized viewport to the video's
  // own source dimensions, so mismatched image/video sizes still line up.
  livePhotoViewport.value = viewport
    ? { ...viewport, isZoomFit: false }
    : null;
  isLivePhotoPlaying.value = true;
}

watch(
  () => [props.file?.id, props.file?.media_subtype, props.file?.modified_at],
  async ([fileId, mediaSubtype]) => {
    const requestSeq = ++motionPhotoVideoRequestSeq;
    isLivePhotoPlaying.value = false;
    livePhotoViewport.value = null;
    motionPhotoVideoPath.value = null;
    // For Motion Photos the embedded MP4 has to be extracted (cached) before
    // the generic video pipeline can play it.
    if (mediaSubtype === 'motion_photo' && fileId != null) {
      try {
        const path = await getMotionPhotoVideoPath(fileId);
        if (requestSeq === motionPhotoVideoRequestSeq) {
          motionPhotoVideoPath.value = path;
        }
      } catch (error) {
        if (requestSeq === motionPhotoVideoRequestSeq) {
          console.error('Failed to prepare motion photo video:', error);
        }
      }
    }
  },
  { immediate: true },
);

const emit = defineEmits([
  'prev', 
  'next', 
  'toggle-slide-show', 
  'update:slideShowIntervalIndex',
  'close', 
  'scale', 
  'update:isZoomFit',
  'zoom-percent',
  'zoom-fit',
  'item-action', 
  'toggle-full-screen', 
  'slideshow-next', 
  'media-dblclick', 
  'viewport-change',
  'view-background-change',
  'context-menu',
  'viewer-more',
  'activate',
  'toggle-af-areas',
]);

const { locale, messages, t } = useI18n();
const localeMsg = computed(() => messages.value[locale.value] as any);

const contextMenuRef = ref<any>(null);
const backgroundContextMenuRef = ref<any>(null);
const containerRef = ref<HTMLElement | null>(null);
const mediaAreaRef = ref<HTMLElement | null>(null);
const imageRef = ref<any>(null);
const videoRef = ref<any>(null);
const imageFile = computed(() => [1, 3].includes(props.file?.file_type) ? props.file : null);
const videoFile = computed(() => props.file?.file_type === 2 ? props.file : null);
const hasImage = ref(false);
const hasVideo = ref(false);
watch(imageFile, file => { if (file) hasImage.value = true; }, { immediate: true });
watch(videoFile, file => { if (file) hasVideo.value = true; }, { immediate: true });
const mediaRef = computed(() => videoFile.value ? videoRef.value : imageFile.value ? imageRef.value : null);

const loadDelay = ref(0);
let lastSwitchAt = -Infinity;
watch(() => props.file?.id, id => {
  if (!id) return;
  const now = performance.now();
  loadDelay.value = now - lastSwitchAt < 150 ? 70 : 0;
  lastSwitchAt = now;
}, { immediate: true });
const reportedDisplayScale = ref<number | null>(null);
const reportedFit = ref<boolean | null>(null);
const displayIsFit = computed(() => (props.toolbarOnly ? props.displayZoomFit : reportedFit.value) ?? props.isZoomFit);
const zoomScaleLabel = computed(() => {
  if (!props.file || props.fileIndex < 0) return '—';
  if (displayIsFit.value) return t('image_viewer.toolbar.zoom_fit_short');
  const displayScale = !props.toolbarOnly && reportedDisplayScale.value !== null
    ? reportedDisplayScale.value
    : props.imageScale * (props.file.file_type === 2 ? 1 : (window.devicePixelRatio || 1));
  return `${Math.round(displayScale * 100)}%`;
});
function handleViewportChange(viewport: any) {
  reportedFit.value = viewport.isZoomFit ?? null;
  emit('viewport-change', viewport);
}
function handleScale(event: any) {
  reportedDisplayScale.value = Number(event.displayScale ?? event.scale);
  emit('scale', event);
}
watch(() => props.file?.id, () => {
  reportedDisplayScale.value = null;
  reportedFit.value = null;
});

const toast = useToast();
const showAfAreas = computed({
  get: () => Boolean(config.mediaViewer.showAfAreas),
  set: (value: boolean) => {
    config.mediaViewer.showAfAreas = value;
    void tauriEmit('af-area-display-changed', value).catch(console.error);
  },
});
const afAreaFeedbackRequest = ref<{ filePath: string; fileVersion: number } | null>(null);
function toggleAfAreas() {
  if (props.fileIndex < 0 || !props.canInteract || ![1, 3].includes(Number(props.file?.file_type))) return;
  if (props.toolbarOnly) {
    emit('toggle-af-areas');
    return;
  }
  const enabled = !showAfAreas.value;
  // Only the clicked pane may notify, and only for the file present at this click.
  afAreaFeedbackRequest.value = enabled ? {
    filePath: props.file?.file_path || '',
    fileVersion: props.file?.modified_at || 0,
  } : null;
  showAfAreas.value = enabled;
}

const isHoverLeft = ref(false);
const isHoverRight = ref(false);
const isHoverTop = ref(false);
const isHoverBottom = ref(false);
const toolbarPosition = ref<'top' | 'bottom'>('top');
const hasOpenMenu = ref(false);

// Responsive toolbar
const containerWidth = ref(0);
const buttonsRef = ref<HTMLElement | null>(null);
const buttonsWidth = ref(0);
const rawControlsWidth = ref(0);
const rawControlsHovered = ref(false);
let observedRawControls: HTMLElement | null = null;
const rawToolbarNeedsTop = computed(() => rawControlsWidth.value > 0
  && containerWidth.value < buttonsWidth.value + 2 * rawControlsWidth.value + 48);

function measureRawControls() {
  const controls = containerRef.value?.querySelector<HTMLElement>('[data-raw-preview-controls]') || null;
  if (controls !== observedRawControls) {
    if (observedRawControls) resizeObserver?.unobserve(observedRawControls);
    observedRawControls = controls;
    if (controls) resizeObserver?.observe(controls);
  }
  rawControlsWidth.value = controls?.offsetWidth || 0;
  if (!rawControlsWidth.value) rawControlsHovered.value = false;
}

onUpdated(measureRawControls);

function handleRawControlsHover(e: MouseEvent) {
  rawControlsHovered.value = !!(e.target as Element)?.closest('[data-raw-preview-controls]');
  if (rawControlsHovered.value) handleMouseLeave();
}
const filenameMaxWidth = computed(() => {
  if (containerWidth.value > 0 && buttonsWidth.value > 0) {
    const val = (containerWidth.value / 2) - (buttonsWidth.value / 2) - 100;
    return Math.max(0, val);
  }
  return 200; // Fallback
});
const showExtraIcons = computed(() => containerWidth.value > 600);
// Window control state (Windows + ImageViewer mode)
const showDesktopWindowControls = isWin || isLinux;
// Preview owns its fullscreen session; the standalone viewer delegates to its window.
const previewFullScreen = ref(false);
const isFullScreen = computed(() => props.isFullScreen || previewFullScreen.value);
let fullscreenBusy = false;
let previewDisposed = false;
let windowWasFullScreen = false;
let windowWasMaximized = false;
let unlistenPreviewResize: (() => void) | undefined;
function stopPreviewResizeListener() {
  unlistenPreviewResize?.();
  unlistenPreviewResize = undefined;
}

async function restorePreviewWindow() {
  const appWindow = getCurrentWindow();
  if (!windowWasFullScreen) {
    if (await appWindow.isFullscreen()) await appWindow.setFullscreen(false);
    if (isWin && windowWasMaximized) {
      await new Promise(resolve => setTimeout(resolve, 80));
      await appWindow.maximize();
    }
  }
  await recoverPreviewWindowFocus();
}

async function exitPreviewFullScreen() {
  return runPreviewWindowOperation(async (cancelFocusRecovery) => {
    if (!previewFullScreen.value) return;
    cancelFocusRecovery();
    fullscreenBusy = true;
    try {
      await restorePreviewWindow();
      previewFullScreen.value = false;
      // Closing a preview restores the window without forgetting its viewing mode.
      if (!previewDisposed) config.mediaViewer.isFullScreen = false;
      stopPreviewResizeListener();
    } catch (error) {
      console.error('Failed to exit preview fullscreen', error);
    } finally {
      fullscreenBusy = false;
    }
  });
}

async function toggleFullScreen() {
  if (props.mode === 2) {
    emit('toggle-full-screen');
    return;
  }
  if (fullscreenBusy) return;
  if (previewFullScreen.value) return exitPreviewFullScreen();
  fullscreenBusy = true;
  return runPreviewWindowOperation(async (cancelFocusRecovery) => {
    if (previewDisposed) { fullscreenBusy = false; return; }
    cancelFocusRecovery();
    stopPreviewResizeListener();
    const appWindow = getCurrentWindow();
    let capturedWindowState = false;
    try {
      windowWasFullScreen = await appWindow.isFullscreen();
      windowWasMaximized = await appWindow.isMaximized();
      capturedWindowState = true;
      if (!windowWasFullScreen) {
        if (isWin && windowWasMaximized) {
          await appWindow.unmaximize();
          await new Promise(resolve => setTimeout(resolve, 80));
        }
        await appWindow.setFullscreen(true);
      }
      if (previewDisposed) {
        await restorePreviewWindow();
        return;
      }
      previewFullScreen.value = true;
      unlistenPreviewResize = await appWindow.onResized(async () => {
        if (!fullscreenBusy && previewFullScreen.value && !(await appWindow.isFullscreen())) {
          void exitPreviewFullScreen();
        }
      });
      if (previewDisposed) {
        unlistenPreviewResize?.();
        await restorePreviewWindow();
      } else {
        config.mediaViewer.isFullScreen = true;
      }
    } catch (error) {
      console.error('Failed to enter preview fullscreen', error);
      unlistenPreviewResize?.();
      if (capturedWindowState) {
        await restorePreviewWindow().catch(error => console.error('Failed to restore preview window', error));
      }
      previewFullScreen.value = false;
    } finally {
      fullscreenBusy = false;
    }
  });
}

const desktopAppWindow = showDesktopWindowControls ? getCurrentWindow() : null;
const isMaximized = ref(false);

const minimizeWindow = () => desktopAppWindow?.minimize();
const toggleMaximizeWindow = () => {
  desktopAppWindow?.isMaximized().then((maximized) => {
    if (maximized) {
      isMaximized.value = false;
      desktopAppWindow?.unmaximize();
    } else {
      isMaximized.value = true;
      desktopAppWindow?.maximize();
    }
  });
};
const closeWindow = () => desktopAppWindow?.close();
const shortcutPlatform: ShortcutPlatform = isMac ? 'mac' : (isLinux ? 'linux' : 'windows');
const shortcut = (actionId: ShortcutActionId) => getShortcutLabel(actionId, shortcutPlatform);
const effectiveSlideShowIntervalIndex = computed(() => {
  return props.slideShowIntervalIndex ?? config.settings.slideShowInterval;
});
const currentSlideShowIntervalLabel = computed(() => `${getSlideShowInterval(effectiveSlideShowIntervalIndex.value)}s`);
const slideShowTransitionMode = computed(() => Number(config.settings.slideShowTransition ?? 0));
const viewBackgroundColors = ['transparent', '#000000', '#333333', '#808080', '#d3d3d3', '#ffffff'];
const viewBackgroundSwatches: CSSProperties[] = [
  { background: 'linear-gradient(135deg, var(--color-base-100) 0 50%, var(--color-base-300) 50% 100%)' },
  ...viewBackgroundColors.slice(1).map(backgroundColor => ({ backgroundColor })),
];
const viewBackgroundStyle = computed(() => {
  const backgroundColor = viewBackgroundColors[Number(config.settings.viewBackground ?? 0)] ?? viewBackgroundColors[0];
  return isFullScreen.value && backgroundColor === 'transparent'
    ? undefined
    : { backgroundColor };
});
const viewBackgroundMenuItems = computed(() => {
  const labels = localeMsg.value.settings.image_view.view_background_options || [];
  const selectedBackground = Number(config.settings.viewBackground ?? 0);
  return [
    {
      label: localeMsg.value.settings.shortcuts.actions.cycle_background,
      icon: IconPalette,
      shortcut: shortcut('view.cycleBackground'),
      action: cycleViewBackground,
    },
    { label: '-', action: null },
    ...VIEW_BACKGROUND_SHORTCUTS.map(({ actionId, value }) => ({
    label: labels[value],
    swatch: viewBackgroundSwatches[value],
    selected: selectedBackground === value,
    shortcut: shortcut(actionId),
    action: () => emit('view-background-change', value),
    })),
  ];
});
const slideShowIntervalOptions = [1, 3, 5, 10, 15, 30];
const slideShowIntervalMenuItems = computed(() => {
  const currentInterval = getSlideShowInterval(effectiveSlideShowIntervalIndex.value);
  return slideShowIntervalOptions.map((seconds, index) => ({
    label: `${seconds}s`,
    icon: currentInterval === seconds ? IconDot : null,
    action: () => {
      if (props.slideShowIntervalIndex !== null) {
        emit('update:slideShowIntervalIndex', index);
      } else {
        config.settings.slideShowInterval = index;
      }
    },
  }));
});

type StatusBadge = {
  key: string;
  icon?: Component;
  icons?: Array<{
    icon: Component;
  }>;
  label?: string;
  iconClass?: string;
  iconStyle?: CSSProperties;
};

const quickViewStatusBadges = computed<StatusBadge[]>(() => {
  const badges: StatusBadge[] = [];
  const metaIcons: StatusBadge['icons'] = [];

  if (props.file?.is_favorite) {
    badges.push({
      key: 'favorite',
      icon: IconHeartFilled,
      iconClass: 'text-error',
    });
  }
  if (props.file?.has_tags) metaIcons.push({ icon: IconTag });
  if (props.file?.has_collections) metaIcons.push({ icon: IconCollection });
  if (props.file?.comments?.length > 0) metaIcons.push({ icon: IconComment });
  if (metaIcons.length > 0) {
    badges.push({
      key: 'meta',
      icons: metaIcons,
    });
  }
  if (isOriginalUnavailable(props.file)) {
    badges.push({ key: 'unavailable', icon: IconExclamation, iconClass: 'text-warning/70' });
  }

  return badges;
});

const statusFeedbackVisible = ref(false);
const unfavoriteFeedback = ref(false);
let statusFeedbackTimer: ReturnType<typeof setTimeout> | undefined;
function sameStatusBadges(badges: StatusBadge[], previous: StatusBadge[]) {
  return badges.length === previous.length && badges.every((badge, index) => {
    const other = previous[index];
    return badge.key === other.key && badge.icon === other.icon && badge.label === other.label
      && badge.iconClass === other.iconClass
      && badge.icons?.length === other.icons?.length
      && (badge.icons || []).every((entry, i) => entry.icon === other.icons![i].icon);
  });
}
function clearStatusFeedback() {
  clearTimeout(statusFeedbackTimer);
  statusFeedbackVisible.value = false;
  unfavoriteFeedback.value = false;
}
watch(
  [quickViewStatusBadges, () => props.file?.id, () => libConfig._libraryId, isFullScreen],
  ([badges, fileId, libraryId, fullscreen], [previousBadges, previousId, previousLibrary, wasFullscreen]) => {
    if (!fullscreen || !wasFullscreen || fileId !== previousId || libraryId !== previousLibrary) {
      clearStatusFeedback();
      return;
    }
    if (sameStatusBadges(badges, previousBadges)) return;
    clearStatusFeedback();
    statusFeedbackVisible.value = true;
    unfavoriteFeedback.value = previousBadges.some(badge => badge.key === 'favorite')
      && !badges.some(badge => badge.key === 'favorite');
    statusFeedbackTimer = setTimeout(clearStatusFeedback, 1200);
  },
);

const fullScreenToolbarVisible = computed(() =>
  props.forceToolbarVisible || (props.showToolbar && (isHoverTop.value || hasOpenMenu.value)),
);
const showStatusBadges = computed(() => isFullScreen.value
  ? fullScreenToolbarVisible.value || statusFeedbackVisible.value
  : props.mode === 0 || props.mode === 2,
);

const showWindowControlsBar = computed(() => {
  return props.showToolbar && !(showDesktopWindowControls && props.mode === 2 && props.isFullScreen);
});
let resizeObserver: ResizeObserver | null = null;

onMounted(() => {
  resizeObserver = new ResizeObserver((entries) => {
    measureRawControls();
    for (const entry of entries) {
      if (entry.target === containerRef.value) {
        containerWidth.value = entry.contentRect.width;
      } else if (entry.target === buttonsRef.value) {
        buttonsWidth.value = entry.contentRect.width;
      }
    }
  });

  if (containerRef.value) {
    resizeObserver.observe(containerRef.value);
  }
  if (buttonsRef.value) {
    resizeObserver.observe(buttonsRef.value);
  }
  // Restore on opening Quick Preview. Merely mounting the embedded filmstrip
  // pane (including at app startup) must not take over the main window.
  if (props.mode === 0 && config.mediaViewer.isFullScreen) {
    void toggleFullScreen();
  }
  void listen(PERSONS_CHANGED_EVENT, () => {
    if (showVideoFaceBar.value && videoFile.value?.id) {
      void loadVideoFaces(Number(videoFile.value.id));
    }
  }).then(unlisten => {
    if (previewDisposed) {
      unlisten();
    } else {
      unlistenVideoPersonsChanged = unlisten;
    }
  }).catch(() => {});
});

onBeforeUnmount(() => {
  clearStatusFeedback();
  previewDisposed = true;
  unlistenVideoPersonsChanged?.();
  unlistenVideoPersonsChanged = null;
  stopPreviewResizeListener();
  void exitPreviewFullScreen();
  if (resizeObserver) {
    resizeObserver.disconnect();
  }
});

function toggleToolbarPin() {
  if (config.mediaViewer.isPinned) {
    toolbarPosition.value = config.mediaViewer.pinnedPosition === 'bottom' ? 'bottom' : 'top';
  } else {
    config.mediaViewer.pinnedPosition = toolbarPosition.value;
  }
  config.mediaViewer.isPinned = !config.mediaViewer.isPinned;
}

function handleMouseMove(e: MouseEvent) {
  handleRawControlsHover(e);
  if (rawControlsHovered.value) return;
  if (!containerRef.value) return;

  const containerRect = containerRef.value.getBoundingClientRect();
  if (containerRect.width <= 0 || containerRect.height <= 0) return;
  const containerY = e.clientY - containerRect.top;
  const containerHeight = containerRect.height;
  if (!config.mediaViewer.isPinned) {
    toolbarPosition.value = containerY < containerHeight * 0.5 ? 'top' : 'bottom';
  }

  if (!mediaAreaRef.value) {
    isHoverTop.value = containerY < 60;
    isHoverBottom.value = containerY > containerHeight - 60;
    return;
  }

  // Prev/next hover logic: based on actual media display area (accounts for panels/toolbar layout).
  const mediaRect = mediaAreaRef.value.getBoundingClientRect();
  if (mediaRect.width <= 0 || mediaRect.height <= 0) {
    isHoverLeft.value = false;
    isHoverRight.value = false;
    isHoverTop.value = containerY < 60;
    isHoverBottom.value = containerY > containerHeight - 60;
    return;
  }

  const mediaX = e.clientX - mediaRect.left;
  const mediaY = e.clientY - mediaRect.top;
  const withinMediaY = mediaY >= 0 && mediaY <= mediaRect.height;
  isHoverLeft.value = withinMediaY && mediaX >= 0 && mediaX < mediaRect.width * 0.1;
  isHoverRight.value = withinMediaY && mediaX <= mediaRect.width && mediaX > mediaRect.width * 0.9;
  const isHoveringNavigation = isHoverLeft.value || isHoverRight.value;
  isHoverTop.value = !isHoveringNavigation && containerY < 60;
  isHoverBottom.value = !isHoveringNavigation && containerY > containerHeight - 60;
}

function handleMouseLeave() {
  isHoverLeft.value = false;
  isHoverRight.value = false;
  isHoverTop.value = false;
  isHoverBottom.value = false;
}

function handleContextMenu(e: MouseEvent) {
  e.stopPropagation();
  if (props.mode === 2) {
    emit('context-menu', e);
    return;
  }
  contextMenuRef.value?.open(e.clientX, e.clientY);
}

function handleBackgroundContextMenu(e: MouseEvent) {
  const target = e.target as HTMLElement;
  if (target.closest('button')) return;
  e.stopPropagation();
  if (props.mode === 2) {
    emit('context-menu', e);
    return;
  }
  backgroundContextMenuRef.value?.open(e.clientX, e.clientY);
}

function cycleViewBackground() {
  config.cycleViewBackground();
  emit('view-background-change', config.settings.viewBackground);
}

const computedToolbarClass = computed(() => {
  const commonClasses = 'absolute z-80 h-10 flex flex-row items-center justify-center select-none';

  if (isFullScreen.value) {
    const floatingClasses = 'left-1/2 top-4 -translate-x-1/2 px-2 rounded-box bg-base-100/30 hover:bg-base-100/70 transition-[opacity,transform] duration-300 ease-in-out';
    return `${commonClasses} ${floatingClasses} ${fullScreenToolbarVisible.value ? 'opacity-100' : 'opacity-0'}`;
  }

  const isPinned = props.mode === 2 ? true : config.mediaViewer.isPinned;

  if (isPinned) {
    // Keep the edge where the floating toolbar was pinned.
    if (props.mode !== 2 && config.mediaViewer.pinnedPosition === 'bottom') {
      return `${commonClasses} relative bottom-0 left-0 w-full order-last`;
    }
    return `${commonClasses} relative top-0 left-0 w-full`;
  } else {
    // Floating Hover Bar
    const floatingClasses = 'left-1/2 -translate-x-1/2 px-2 rounded-box bg-base-100/30 hover:bg-base-100/70 transition-[opacity,transform] duration-300 ease-in-out';
    
    if (toolbarPosition.value === 'bottom' && !rawToolbarNeedsTop.value) {
       if (rawControlsHovered.value && !hasOpenMenu.value) {
         return `${commonClasses} ${floatingClasses} bottom-4 invisible opacity-0 pointer-events-none`;
       }
       if (isHoverBottom.value || hasOpenMenu.value) {
          if (props.file.file_type === 2) {
            return `${commonClasses} ${floatingClasses} bottom-8 opacity-100`;
          } else {
            return `${commonClasses} ${floatingClasses} bottom-4 opacity-100`;
          }
       } else {
          if (props.file.file_type === 2) {
            return `${commonClasses} ${floatingClasses} bottom-8 opacity-0`;
          } else {
            return `${commonClasses} ${floatingClasses} bottom-4 opacity-0`;
          }
       }
    } else {
       if (isHoverTop.value || (rawToolbarNeedsTop.value && isHoverBottom.value) || hasOpenMenu.value) {
          return `${commonClasses} ${floatingClasses} top-4 opacity-100`;
       } else {
          return `${commonClasses} ${floatingClasses} top-4 opacity-0`;
       }
    }
  }
});

const handleMenuOpenChange = (isOpen: boolean) => {
  hasOpenMenu.value = isOpen;
};

// Expose methods for parent component (ImageViewer)
const zoomToPercent = (percent: number) => {
  if (props.toolbarOnly) emit('zoom-percent', percent);
  else mediaRef.value?.zoomToPercent?.(percent);
};
const zoomToFit = () => {
  if (props.toolbarOnly) emit('zoom-fit');
  else mediaRef.value?.zoomToFit?.();
};
const zoomPercentMenuItems = computed(() => [
  { label: t('image_viewer.toolbar.zoom_fit'), icon: displayIsFit.value ? IconDot : null, shortcut: shortcut('view.zoomFit'), action: zoomToFit },
  { label: '-' },
  ...[10, 25, 50, 75, 100, 125, 150, 200, 400, 800].map(percent => ({
    label: `${percent}%`, icon: zoomScaleLabel.value === `${percent}%` ? IconDot : null,
    action: () => zoomToPercent(percent),
  })),
]);
const toggleZoomFit = () => {
  if (mediaRef.value?.toggleZoomFit) mediaRef.value.toggleZoomFit();
  else emit('update:isZoomFit', !props.isZoomFit);
};

const zoomIn = () => mediaRef.value?.zoomIn();
const zoomOut = () => mediaRef.value?.zoomOut();
const zoomActual = () => mediaRef.value?.zoomActual();
const rotateView = (delta = 90) => mediaRef.value?.rotateView(delta);
const togglePlay = () => mediaRef.value?.togglePlay?.();
const getViewportState = () => mediaRef.value?.getViewportState?.();
const applyViewportState = (viewport: any, silent = false) => {
  mediaRef.value?.applyViewportState?.(viewport, silent);
  // Silent synchronization suppresses renderer events; publish its resulting
  // mode so the local toolbar and the active-pane toolbar stay up to date.
  if (silent) {
    const applied = mediaRef.value?.getViewportState?.();
    if (applied) handleViewportChange(applied);
  }
};
const getCurrentImageSrc = () => mediaRef.value?.getCurrentImageSrc?.() || '';
const clearPreloadCache = (filePath?: string) => imageRef.value?.clearPreloadCache?.(filePath);
const showMessage = (message: string, isWarning: boolean = false) => {
  if (isWarning) {
    toast.warning(message, { placement: 'bottom-right' });
    return;
  }
  toast.info(message, { placement: 'bottom-right' });
};

const triggerPrev = () => {
  if (props.hasPrevious) {
    emit('prev');
  } else {
    showMessage((localeMsg.value as any).tooltip.image_viewer.first_image);
  }
}

const triggerNext = () => {
  if (props.hasNext) {
    emit('next');
  } else {
    showMessage((localeMsg.value as any).tooltip.image_viewer.last_image);
  }
}

const handleToggleSlideShow = () => {
  if (!props.isSlideShow) {
    emit('update:isZoomFit', true);
  }
  emit('toggle-slide-show');
}

const handleZoomIn = () => {
  if (props.toolbarOnly || !mediaRef.value) {
    emit('item-action', { action: 'zoom-in', index: props.fileIndex });
    return;
  }
  zoomIn();
};

const handleZoomOut = () => {
  if (props.toolbarOnly || !mediaRef.value) {
    emit('item-action', { action: 'zoom-out', index: props.fileIndex });
    return;
  }
  zoomOut();
};

const handleOverlayClick = () =>{
  if(props.mode === 0){
    emit('close')
  }
}

let overlayPointer: { id: number; x: number; y: number } | null = null;

const resetOverlayPointer = () => {
  overlayPointer = null;
};

const isOverlayBackdropTarget = (event: PointerEvent) => {
  const target = event.target;
  const currentTarget = event.currentTarget;

  if (!(currentTarget instanceof HTMLElement)) return false;
  if (!(target instanceof Element)) return false;
  if (target === currentTarget) return true;

  if (target.closest(
    'img, video, button, a, input, select, textarea, label, [role="button"], [data-raw-preview-controls], [data-image-navigator], [data-video-play-overlay], .vjs-tech, .vjs-control-bar, .vjs-control, .vjs-big-play-button, .vjs-menu'
  )) {
    return false;
  }

  return currentTarget.contains(target);
};

const handleOverlayPointerDown = (event: PointerEvent) => {
  if ((event.pointerType === 'mouse' || event.pointerType === 'pen') && event.button !== 0) {
    resetOverlayPointer();
    return;
  }
  const isBackdropTarget = isOverlayBackdropTarget(event);
  if (hasOpenMenu.value && isBackdropTarget) {
    resetOverlayPointer();
    return;
  }
  overlayPointer = isBackdropTarget
    ? { id: event.pointerId, x: event.clientX, y: event.clientY }
    : null;
};

const handleOverlayPointerUp = (event: PointerEvent) => {
  if ((event.pointerType === 'mouse' || event.pointerType === 'pen') && event.button !== 0) {
    resetOverlayPointer();
    return;
  }
  const pointer = overlayPointer;
  resetOverlayPointer();
  if (
    !pointer
    || pointer.id !== event.pointerId
    || !isOverlayBackdropTarget(event)
    || Math.abs(event.clientX - pointer.x) > OVERLAY_CLICK_MOVE_TOLERANCE
    || Math.abs(event.clientY - pointer.y) > OVERLAY_CLICK_MOVE_TOLERANCE
  ) {
    return;
  }
  handleOverlayClick();
};

const handleMessageFromImageViewer = (payload: { message: string }) => {
  if (payload.message === 'prev') {
    triggerPrev();
  } else if (payload.message === 'next') {
    triggerNext();
  } else if (payload.message === 'close') {
    handleOverlayClick();
  }
};

defineExpose({
  isFullScreen,
  exitPreviewFullScreen,
  toggleFullScreen,
  zoomIn,
  zoomOut,
  zoomActual,
  rotateView,
  togglePlay,
  toggleAfAreas,
  cycleRawPreview: () => !isLivePhotoPlaying.value && mediaRef.value?.cycleRawPreview?.(),
  getViewportState,
  applyViewportState,
  navigateImage: (point: { normX: number; normY: number }) => mediaRef.value?.navigateImage?.(point),
  zoomNavigator: (factor: number) => mediaRef.value?.zoomNavigator?.(factor),
  toggleZoomFit,
  zoomToPercent,
  zoomToFit,
  getCurrentImageSrc,
  clearPreloadCache,
  showMessage,
  triggerPrev,
  triggerNext
});

const originalUnavailable = computed(() => isOriginalUnavailable(props.file));
watch(originalUnavailable, unavailable => {
  if (unavailable) isLivePhotoPlaying.value = false;
});
watch(() => [!props.toolbarOnly && props.file?.file_path, libConfig._libraryId] as const, ([path, libraryId], _previous, onCleanup) => {
  if (!path) return;
  let cancelled = false;
  const timer = setTimeout(async () => {
    try {
      const available = await checkFileAccessibility(path);
      if (!cancelled && libraryId === libConfig._libraryId) setFileAccessibility(path, available);
    } catch { /* An IPC failure is not evidence that the original is offline. */ }
  }, 150);
  onCleanup(() => {
    clearTimeout(timer);
    cancelled = true;
  });
}, { immediate: true });

const selectedFile = computed(() => props.file);

interface VideoFaceTag {
  id: number;
  file_id: number;
  person_id: number | null;
  person_name: string | null;
  kind: string;
  manual: boolean;
  thumbnail: string;
}

const videoFaces = ref<VideoFaceTag[]>([]);
const videoAssignFace = ref<AssignFaceTarget | null>(null);
const isScanningVideoFaces = ref(false);
let videoFacesRequestSeq = 0;
let unlistenVideoPersonsChanged: (() => void) | null = null;

const showVideoFaceBar = computed(() =>
  Boolean(
    videoFile.value?.id
    && config.settings.face.enabled
    && config.mediaViewer.showFaces
    && !props.isSlideShow
    && !props.toolbarOnly
  ),
);

async function loadVideoFaces(fileId: number) {
  const seq = ++videoFacesRequestSeq;
  if (!fileId || fileId <= 0) {
    if (seq === videoFacesRequestSeq) videoFaces.value = [];
    return;
  }
  const faces = await getFacesForFile(fileId);
  if (seq !== videoFacesRequestSeq) return;
  const loaded = await Promise.all(
    (faces || []).map(async (face: any) => {
      let parsedBbox: any = null;
      try {
        parsedBbox = face.bbox ? JSON.parse(face.bbox) : null;
      } catch {
        parsedBbox = null;
      }
      const hasBox = Boolean(parsedBbox && Number(parsedBbox.width) > 0 && Number(parsedBbox.height) > 0);
      const personId = Number(face.person_id || 0) || null;
      let thumbnail = hasBox ? ((await getFaceThumbnail(Number(face.id))) || '') : '';
      if (!thumbnail && personId) {
        thumbnail = (await getPersonThumbnail(personId)) || '';
      }
      return {
        id: Number(face.id),
        file_id: fileId,
        person_id: personId,
        person_name: face.person_name || null,
        kind: face.kind || 'face',
        manual: Boolean(parsedBbox?.manual || !hasBox),
        thumbnail,
      };
    }),
  );
  if (seq !== videoFacesRequestSeq) return;
  videoFaces.value = loaded.sort((a, b) => Number(!a.person_id) - Number(!b.person_id));
}

watch(
  () => [showVideoFaceBar.value, Number(videoFile.value?.id || 0)] as const,
  ([visible, fileId]) => {
    if (!visible || !fileId) {
      videoFacesRequestSeq++;
      videoFaces.value = [];
      return;
    }
    void loadVideoFaces(fileId);
  },
  { immediate: true },
);

function openVideoAssignFace(face: VideoFaceTag) {
  videoAssignFace.value = {
    id: face.id,
    file_id: face.file_id,
    person_id: face.person_id,
    person_name: face.person_name,
    thumbnail: face.thumbnail,
    kind: face.kind,
    manual: face.manual,
  };
}

function openAddVideoManualFace() {
  const fileId = Number(videoFile.value?.id || 0);
  if (!fileId) return;
  videoAssignFace.value = {
    id: null,
    file_id: fileId,
    person_id: null,
    person_name: null,
    thumbnail: '',
    kind: config.main.sidebarIndex === SIDEBAR.PERSON && config.settings.personKind === 'pet' ? 'pet' : 'person',
    manual: true,
    norm_bbox: null,
  };
}

async function handleScanCurrentVideo() {
  const fileId = Number(videoFile.value?.id || 0);
  if (!fileId || isScanningVideoFaces.value) return;
  isScanningVideoFaces.value = true;
  try {
    const res: any = await scanFileFaces(fileId);
    const added = Number(res?.added || 0);
    const matched = Number(res?.matched || 0);
    if (added > 0) {
      void tauriEmit(PERSONS_CHANGED_EVENT, { fileId });
      await loadVideoFaces(fileId);
      toast.success(
        matched > 0
          ? t('menu.person.scan_photo_found_matched', { count: added, matched })
          : t('menu.person.scan_photo_found', { count: added }),
      );
    } else {
      toast.info(t('menu.person.scan_photo_none'));
    }
  } catch (error) {
    console.error('Failed to scan video for faces:', error);
    toast.error(t('menu.person.scan_video_failed'));
  } finally {
    isScanningVideoFaces.value = false;
  }
}

function onVideoFaceChanged(payload: { personId: number | null; message: string }) {
  videoAssignFace.value = null;
  toast.success(payload.message);
  const fileId = Number(videoFile.value?.id || 0);
  if (fileId) void loadVideoFaces(fileId);
}

const singleFileMenuItems = computed(() => {
  if (props.mode === 2) return [];

  return useFileMenuItems(
    selectedFile,
    localeMsg,
    isMac,
    t,
    (action) => emit('item-action', { action, index: props.fileIndex })
  ).value;
});
</script>

<style scoped>
.play-interval-slot { width: 56px; overflow: hidden; }
.play-interval-enter-active,
.play-interval-leave-active { transition: width 200ms ease, opacity 200ms ease; }
.play-interval-enter-from,
.play-interval-leave-to { width: 0; opacity: 0; }
@media (prefers-reduced-motion: reduce) {
  .play-interval-enter-active,
  .play-interval-leave-active { transition: none; }
}

/* Disable text selection while dragging */
* {
  user-select: none;
}
 
@media (max-width: 600px) {
  #responsiveDiv {
    visibility: hidden;
  }
}
@media (min-width: 600px) {
  #responsiveDiv {
    visibility: visible;
  }
}
</style>
