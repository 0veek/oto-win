<script lang="ts">
  import { IconMinus, IconX } from "@tabler/icons-svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  // The settings window runs with `decorations: false`, so every full-window
  // surface has to draw its own chrome — including onboarding and the boot
  // error screen, which render outside the settings shell. Without this the
  // window cannot be dragged or dismissed at all on those screens.
  //
  // Both calls are guarded because the same pages render in a plain browser
  // during layout checks, where the Tauri API is absent.
  async function minimizeWindow() {
    try {
      await getCurrentWindow().minimize();
    } catch {
      // Browser preview without Tauri.
    }
  }

  async function closeWindow() {
    try {
      // The backend intercepts CloseRequested and hides the settings window.
      await getCurrentWindow().close();
    } catch {
      // Browser preview without Tauri.
    }
  }
</script>

<header class="titlebar" data-tauri-drag-region>
  <span class="plate-micro titlebar__title" data-tauri-drag-region>Oto</span>
  <div class="titlebar__controls">
    <button type="button" class="titlebar__btn" aria-label="Minimize" onclick={minimizeWindow}>
      <IconMinus aria-hidden="true" size={14} stroke={1.8} />
    </button>
    <button
      type="button"
      class="titlebar__btn titlebar__btn--close"
      aria-label="Close"
      onclick={closeWindow}
    >
      <IconX aria-hidden="true" size={14} stroke={1.8} />
    </button>
  </div>
</header>
