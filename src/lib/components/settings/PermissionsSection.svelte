<script lang="ts">
  import { IconArrowRight, IconMicrophone, IconPointer } from "@tabler/icons-svelte";

  let {
    onselect,
  }: {
    /** Navigate to another settings section without a full page remount. */
    onselect?: (id: string) => void;
  } = $props();
</script>

<section class="section">
  <header class="section__head">
    <h2 class="section__title">Permissions</h2>
    <p class="section__lead">
      What Oto needs from Windows to hear you and to put text where you were typing.
      Both are controlled by Windows itself, not from inside Oto.
    </p>
  </header>

  <div class="pairs">
    <div class="pair">
      <span class="pair__icon">
        <IconMicrophone aria-hidden="true" size={16} stroke={1.7} />
      </span>
      <span class="pair__copy">
        <strong>Microphone</strong>
        <span>
          Needed the whole time Oto is listening. Windows governs it under
          <span class="readout-tight">Settings → Privacy &amp; security → Microphone</span> — Oto
          never raises its own prompt. If dictation records silence, check that microphone access
          and <em>Let desktop apps access your microphone</em> are turned on there.
        </span>
      </span>
      <button type="button" class="btn-link pair__side" onclick={() => onselect?.("audio")}>
        Test microphone
        <IconArrowRight aria-hidden="true" size={14} stroke={1.8} />
      </button>
    </div>

    <div class="pair">
      <span class="pair__icon">
        <IconPointer aria-hidden="true" size={16} stroke={1.7} />
      </span>
      <span class="pair__copy">
        <strong>Typing into other windows</strong>
        <span>
          Oto sends keystrokes through Windows' own input API, which needs no permission for
          ordinary applications. Windows does block input sent to a window running as
          Administrator, so elevated apps, the UAC prompt and the sign-in screen cannot receive
          dictated text — run Oto at the same privilege level, or paste from the clipboard.
        </span>
      </span>
      <button type="button" class="btn-link pair__side" onclick={() => onselect?.("injection")}>
        Check insertion
        <IconArrowRight aria-hidden="true" size={14} stroke={1.8} />
      </button>
    </div>
  </div>
</section>
