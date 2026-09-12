<script lang="ts">
  import { onMount } from "svelte";
  import AvatarShell from "$lib/views/AvatarShell.svelte";
  import ChatPanel from "$lib/views/ChatPanel.svelte";
  import Dashboard from "$lib/views/Dashboard.svelte";

  type Mode = "loading" | "avatar" | "chat" | "main";
  let mode = $state<Mode>("loading");

  onMount(() => {
    (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const label = getCurrentWindow().label;
        if (label === "avatar") mode = "avatar";
        else if (label === "chat") mode = "chat";
        else mode = "main";
      } catch {
        // Browser / preview fallback
        mode = "main";
      }
      document.documentElement.classList.remove(
        "shell-avatar",
        "shell-avatar-glass",
        "shell-chat",
        "shell-main",
      );
      if (mode === "avatar") {
        document.documentElement.classList.add("shell-avatar");
        if (!/Linux/i.test(navigator.userAgent)) {
          document.documentElement.classList.add("shell-avatar-glass");
        }
      } else if (mode === "chat") document.documentElement.classList.add("shell-chat");
      else document.documentElement.classList.add("shell-main");
    })();
  });
</script>

{#if mode === "loading"}
  <div class="boot-wait"></div>
{:else if mode === "avatar"}
  <AvatarShell />
{:else if mode === "chat"}
  <ChatPanel />
{:else}
  <Dashboard />
{/if}

<style>
  .boot-wait {
    min-height: 100vh;
    background: transparent;
  }
</style>
