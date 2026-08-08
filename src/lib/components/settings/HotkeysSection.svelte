<script lang="ts">
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { IconKeyboard } from "@tabler/icons-svelte";
  import type { AppConfig } from "$lib/types";

  let {
    config = $bindable(),
  }: {
    config: AppConfig;
  } = $props();

  /** True when the Run key says Oto launches at sign-in but config disagrees. */
  let autostartDrifted = $state(false);

  onMount(() => {
    // The startup entry can be removed outside Oto — Task Manager's Startup tab,
    // or a reinstall to a different path. Reconcile the saved flag against what
    // Windows actually holds, so the checkbox never claims something untrue.
    void invoke<boolean>("autostart_active")
      .then((active) => {
        autostartDrifted = active !== config.autostart_enabled;
      })
      .catch(() => {
        // Browser preview or plugin unavailable.
        autostartDrifted = false;
      });
  });

  // A chord reads as keys, not as a string, so it is split for the cap preview.
  const chordKeys = $derived(
    config.hotkey
      .split("+")
      .map((part) => part.trim())
      .filter(Boolean),
  );
</script>

<section class="section">
  <header class="section__head">
    <h2 class="section__title">Hotkeys</h2>
    <p class="section__lead">
      The chord that starts a dictation from anywhere. Hold it, speak, release.
      Windows registers the chord through the system global-shortcut API, so it fires
      no matter which application has focus.
    </p>
  </header>

  <p class="note">
    <IconKeyboard aria-hidden="true" size={16} stroke={1.7} />
    <span>
      Saving below registers your chord system-wide. If Windows or another running
      application already owns it, the registration fails and Oto puts your last working
      shortcut back.
    </span>
  </p>

  <div class="rack">
    <div class="rack__head">
      <span class="plate-micro rack__title">Shortcut</span>
    </div>

    <label class="row">
      <span class="row__label">Dictation chord</span>
      <span class="row__control">
        <input
          type="text"
          class="field-data"
          placeholder="Ctrl+Shift+Space"
          spellcheck="false"
          autocomplete="off"
          bind:value={config.hotkey}
        />
        {#if chordKeys.length}
          <span class="keys" aria-hidden="true">
            {#each chordKeys as key, index (index)}
              {#if index > 0}<span class="keys__join">+</span>{/if}
              <kbd class="key">{key}</kbd>
            {/each}
          </span>
        {/if}
        <span class="row__hint">
          Modifiers plus one key, joined by <span class="readout-tight">+</span>. Ctrl, Alt,
          Shift and Win are accepted, along with Space, Enter, Tab, Escape and a–z.
        </span>
        <span class="row__hint">
          Good starting points: <span class="readout-tight">Ctrl+Shift+Space</span> or
          <span class="readout-tight">Ctrl+Alt+D</span>.
        </span>
      </span>
    </label>

    <div class="row row--stacked row--flush">
      <span class="row__label">Mode chords</span>
      <p class="note row__control">
        <span>
          Modes can each claim their own chord and register the same way. A Mode chord that
          will not bind is skipped with a log line rather than failing the save — ordinary
          dictation keeps working on the chord above.
        </span>
      </p>
    </div>

    <div class="row row--stacked row--flush">
      <span class="row__label">Troubleshooting</span>
      <div class="row__control">
        <details class="disclosure">
          <summary>The overlay never appears</summary>
          <div class="disclosure__body">
            <ul>
              <li>Save after changing the chord — nothing binds until you do.</li>
              <li>
                If the save reports that the chord could not be registered, another
                application already holds it. Pick a different one; Oto keeps the previous
                shortcut bound in the meantime.
              </li>
              <li>
                Skip chords Windows reserves:
                <span class="readout-tight">Win+Space</span> (input language),
                <span class="readout-tight">Win+H</span> (voice typing),
                <span class="readout-tight">Win+L</span>,
                <span class="readout-tight">Alt+Tab</span> and
                <span class="readout-tight">Ctrl+Alt+Delete</span>.
              </li>
              <li>
                <span class="readout-tight">Ctrl+Shift+Space</span> and
                <span class="readout-tight">Ctrl+Alt+D</span> are reliable choices.
              </li>
              <li>
                A window that takes the foreground mid-chord — a UAC prompt, a full-screen
                game, a remote-desktop session — can swallow the key-up. Oto finishes the
                take on its own rather than dropping the words.
              </li>
              <li>The tray's <strong>Start listening</strong> works without any key grab.</li>
            </ul>
          </div>
        </details>
      </div>
    </div>
  </div>

  <div class="rack">
    <div class="rack__head">
      <span class="plate-micro rack__title">Startup</span>
    </div>

    <label class="row row--switch" class:row--flush={!autostartDrifted}>
      <span class="row__copy">
        <strong>Start with Windows</strong>
        <span>
          Adds a startup entry under
          <span class="readout-tight">HKCU\Software\Microsoft\Windows\CurrentVersion\Run</span>,
          so the notification-area icon and the shortcut are ready without opening this window. A
          sign-in launch stays in the notification area — open Settings from there.
        </span>
      </span>
      <input type="checkbox" bind:checked={config.autostart_enabled} />
    </label>

    {#if autostartDrifted}
      <p class="note note--warn" role="status">
        Windows and this setting disagree — something outside Oto changed the startup entry. Save to
        reapply what is shown here.
      </p>
    {/if}
  </div>
</section>
