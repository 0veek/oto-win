<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import type { AppConfig, InjectionMode } from "$lib/types";

  let {
    config = $bindable(),
  }: {
    config: AppConfig;
  } = $props();

  let testBusy = $state(false);
  let testResult = $state<string | null>(null);
  let testError = $state<string | null>(null);

  const MODES: { value: InjectionMode; label: string; hint: string }[] = [
    {
      value: "auto",
      label: "Auto",
      hint: "Clipboard + Ctrl+V first, direct typing if that fails, then clipboard only.",
    },
    {
      value: "direct_type",
      label: "Direct type",
      hint: "Type character-by-character as synthetic Unicode key events (slower on long text).",
    },
    {
      value: "clipboard_paste",
      label: "Clipboard + paste",
      hint: "Always copy, then simulate Ctrl+V.",
    },
    {
      value: "clipboard_only",
      label: "Clipboard only",
      hint: "Copy text and prompt you to paste (Ctrl+V).",
    },
  ];

  async function testInjection() {
    testBusy = true;
    testResult = null;
    testError = null;
    try {
      // Persist mode first so the command reads the selection.
      await invoke("set_config", { cfg: config });
      testResult = await invoke<string>("test_injection");
    } catch (e) {
      testError = String(e);
    } finally {
      testBusy = false;
    }
  }
</script>

<section class="section">
  <header class="section__head">
    <h2 class="section__title">Insertion</h2>
    <p class="section__lead">
      How finished text reaches the window you were working in. Auto works almost
      everywhere; the rest are for when a particular application is stubborn.
    </p>
  </header>

  <div class="rack">
    <div class="rack__head">
      <span class="plate-micro rack__title">Method</span>
    </div>

    <div class="row row--stacked" role="radiogroup" aria-label="How to insert">
      <span class="row__label">How to insert</span>
      <div class="row__control choice-list">
        {#each MODES as mode (mode.value)}
          <label class="choice" data-active={config.injection_mode === mode.value}>
            <input
              type="radio"
              name="injection_mode"
              value={mode.value}
              checked={config.injection_mode === mode.value}
              onchange={() => {
                config.injection_mode = mode.value;
              }}
            />
            <span class="choice__copy">
              <strong>{mode.label}</strong>
              <span>{mode.hint}</span>
            </span>
          </label>
        {/each}
      </div>
    </div>

    <div class="row row--stacked row--flush">
      <span class="row__label">How it works</span>
      <p class="note row__control">
        <span>
          Oto brings the window you dictated from back to the foreground, then sends keystrokes
          with the Win32 <span class="readout-tight">SendInput</span> API — no extra software to
          install. Whatever you had copied is put back on the clipboard about a second later,
          unless you changed it in the meantime.
        </span>
      </p>
    </div>

    <div class="row row--stacked row--flush">
      <span class="row__label">Elevated windows</span>
      <p class="note note--warn row__control">
        <span>
          Windows will not let a normal application send input to a window running as
          Administrator, so insertion into elevated apps, the UAC prompt and the sign-in screen
          is blocked. Run Oto at the same privilege level as the target app, or use
          <strong>Clipboard only</strong> and paste by hand.
        </span>
      </p>
    </div>
  </div>

  <div class="rack">
    <div class="rack__head">
      <span class="plate-micro rack__title">Test</span>
      <p class="rack__note">
        Start the test, then click into a text field in another application. Oto waits a moment and
        inserts <span class="readout-tight">Oto injection test</span> using the method above.
      </p>
    </div>

    <div class="row row--switch row--flush">
      <span class="row__copy">
        <strong>Insert test text</strong>
        <span>Uses the exact path a real dictation would take.</span>
      </span>
      <button type="button" class="btn" disabled={testBusy} onclick={testInjection}>
        {testBusy ? "Inserting…" : "Run test"}
      </button>
    </div>

    {#if testResult}
      <p aria-live="polite" class="note note--ok test-note">{testResult}</p>
    {/if}
    {#if testError}
      <p role="alert" class="note note--bad test-note">{testError}</p>
    {/if}
  </div>
</section>

<style>
  .test-note {
    margin-block-start: var(--space-sm);
  }
</style>
