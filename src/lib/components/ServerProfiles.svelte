<script lang="ts">
  import { t } from "../../i18n";
  import type { Profile } from "../../types";

  interface Props {
    profiles: Profile[];
    activeProfileId: number | null;
    isStreaming: boolean;
    onSelectProfile: (id: number) => void;
    onEditProfile: (profile: Profile) => void;
    onDeleteProfile: (id: number) => void;
    onNewProfile: () => void;
  }

  let {
    profiles,
    activeProfileId,
    isStreaming,
    onSelectProfile,
    onEditProfile,
    onDeleteProfile,
    onNewProfile,
  }: Props = $props();
</script>

<div class="card profiles-card">
  <div class="profiles-header">
    <div class="header-left">
      <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <rect x="2" y="2" width="20" height="8" rx="2" ry="2"/>
        <rect x="2" y="14" width="20" height="8" rx="2" ry="2"/>
        <line x1="6" y1="6" x2="6.01" y2="6"/>
        <line x1="6" y1="18" x2="6.01" y2="18"/>
      </svg>
      <span class="card-title">{t("profilesTitle")}</span>
    </div>

    <button class="btn-new" disabled={isStreaming} onclick={onNewProfile}>
      <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.5">
        <line x1="12" y1="5" x2="12" y2="19"/>
        <line x1="5" y1="12" x2="19" y2="12"/>
      </svg>
      <span>{t("btnNewProfile")}</span>
    </button>
  </div>

  {#if profiles.length === 0}
    <div class="empty-state">
      <p>{t("profileEmptyList")}</p>
    </div>
  {:else}
    <div class="profiles-list">
      {#each profiles as p (p.id)}
        <div
          class="profile-item {p.id === activeProfileId ? 'active' : ''}"
          role="button"
          tabindex="0"
          onclick={() => {
            if (!isStreaming && p.id !== undefined) onSelectProfile(p.id);
          }}
          onkeydown={(e) => {
            if (e.key === "Enter" && !isStreaming && p.id !== undefined) onSelectProfile(p.id);
          }}
        >
          <div class="profile-radio">
            <input
              type="radio"
              name="active_profile"
              checked={p.id === activeProfileId}
              disabled={isStreaming}
              onchange={() => { if (p.id !== undefined) onSelectProfile(p.id); }}
            />
          </div>

          <div class="profile-details">
            <div class="name-row">
              <span class="profile-name">{p.name}</span>
              {#if p.id === activeProfileId}
                <span class="active-tag">{t("profileSelect")}</span>
              {/if}
            </div>
            <div class="meta-row">
              <span class="meta-badge proto-badge">{p.protocol}</span>
              <span class="meta-badge bitrate-badge">{p.bitrate} kbps</span>
              <span class="server-endpoint font-mono">{p.server}:{p.port}{p.mount ? p.mount : ""}</span>
            </div>
          </div>

          <div class="profile-actions">
            <button
              class="btn-icon"
              disabled={isStreaming}
              title={t("btnEditProfile")}
              onclick={(e) => {
                e.stopPropagation();
                onEditProfile(p);
              }}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <path d="M17 3a2.828 2.828 0 1 1 4 4L7.5 20.5 2 22l1.5-5.5L17 3z"/>
              </svg>
            </button>

            <button
              class="btn-icon danger"
              disabled={isStreaming}
              title={t("btnDeleteProfile")}
              onclick={(e) => {
                e.stopPropagation();
                if (p.id !== undefined && confirm(t("confirmDeleteProfile", { name: p.name }))) {
                  onDeleteProfile(p.id);
                }
              }}
            >
              <svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                <polyline points="3 6 5 6 21 6"/>
                <path d="M19 6v14a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V6m3 0V4a2 2 0 0 1 2-2h4a2 2 0 0 1 2 2v2"/>
              </svg>
            </button>
          </div>
        </div>
      {/each}
    </div>
  {/if}
</div>

<style>
  .profiles-card {
    gap: 12px;
  }

  .profiles-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }

  .header-left {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    font-weight: 700;
    color: var(--color-text-main);
  }

  .card-title {
    font-size: 14px;
    font-weight: 700;
  }

  .btn-new {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 12px;
    background-color: var(--color-page);
    border: 1px solid var(--color-border);
    border-radius: 6px;
    font-size: 12px;
    font-weight: 600;
    color: var(--color-text-main);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .btn-new:hover:not(:disabled) {
    background-color: var(--color-primary-soft);
    border-color: var(--color-primary);
    color: var(--color-primary-text);
  }

  .btn-new:disabled {
    opacity: 0.5;
    cursor: not-allowed;
  }

  .empty-state {
    padding: 24px 16px;
    text-align: center;
    font-size: 13px;
    color: var(--color-text-muted);
    background-color: var(--color-page);
    border-radius: 8px;
    border: 1px dashed var(--color-border);
  }

  .profiles-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }

  .profile-item {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 10px 12px;
    border: 1px solid var(--color-border);
    border-radius: 8px;
    background-color: var(--color-page);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .profile-item:hover {
    border-color: var(--color-text-dim);
    background-color: white;
  }

  .profile-item.active {
    border-color: var(--color-primary);
    background-color: white;
    box-shadow: 0 0 0 1px var(--color-primary);
  }

  .profile-radio input {
    accent-color: var(--color-primary);
    cursor: pointer;
  }

  .profile-details {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .name-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .profile-name {
    font-size: 14px;
    font-weight: 600;
    color: var(--color-text-main);
  }

  .active-tag {
    font-size: 10px;
    font-weight: 700;
    text-transform: uppercase;
    background-color: var(--color-primary-soft);
    color: var(--color-primary-text);
    padding: 1px 6px;
    border-radius: 4px;
  }

  .meta-row {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 11px;
  }

  .meta-badge {
    padding: 2px 6px;
    border-radius: 4px;
    font-weight: 600;
  }

  .proto-badge {
    background-color: var(--color-muted-bg);
    color: var(--color-text-muted);
  }

  .bitrate-badge {
    background-color: #e0f2fe;
    color: #0369a1;
  }

  .server-endpoint {
    color: var(--color-text-muted);
    font-size: 11px;
  }

  .profile-actions {
    display: flex;
    gap: 4px;
  }

  .btn-icon {
    width: 28px;
    height: 28px;
    border-radius: 6px;
    border: 1px solid var(--color-border);
    background: transparent;
    display: flex;
    align-items: center;
    justify-content: center;
    color: var(--color-text-muted);
    cursor: pointer;
    transition: all 0.15s ease;
  }

  .btn-icon:hover:not(:disabled) {
    background-color: var(--color-muted-bg);
    color: var(--color-text-main);
  }

  .btn-icon.danger:hover:not(:disabled) {
    background-color: var(--color-danger-soft);
    color: var(--color-danger);
    border-color: var(--color-danger);
  }

  .btn-icon:disabled {
    opacity: 0.4;
    cursor: not-allowed;
  }

  .font-mono {
    font-family: var(--font-mono);
  }
</style>
