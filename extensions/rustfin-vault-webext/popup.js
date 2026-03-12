function $(id) {
  return document.getElementById(id);
}

function callBackground(message) {
  return chrome.runtime.sendMessage(message);
}

function setVisible(id, visible) {
  $(id).classList.toggle('hidden', !visible);
}

async function render() {
  const response = await callBackground({ type: 'get-popup-state' });
  if (!response.ok) {
    $('status').textContent = response.error || 'Failed to load extension state';
    return;
  }

  $('server-url').value = response.settings.serverBaseUrl || '';
  setVisible('pairing-panel', !response.paired);
  setVisible('unlock-panel', response.paired);
  setVisible('matches-panel', response.unlocked);
  setVisible('pending-panel', Boolean(response.pendingSave));

  $('status').textContent = response.unlocked
    ? `Unlocked for ${response.currentTab?.url || 'current tab'}`
    : response.paired
      ? 'Paired but locked'
      : 'Not paired';

  const matchesRoot = $('matches');
  matchesRoot.innerHTML = '';
  for (const match of response.matches || []) {
    const wrapper = document.createElement('div');
    wrapper.className = 'match';
    wrapper.innerHTML = `
      <strong>${match.summary.title}</strong>
      <div class="muted">${match.summary.primary_uri || match.summary.subtitle}</div>
      <div class="muted">${match.summary.username || match.summary.login_email || 'No username'}</div>
    `;
    const fill = document.createElement('button');
    fill.className = 'btn btn-primary';
    fill.textContent = 'Fill on page';
    fill.addEventListener('click', async () => {
      const result = await callBackground({
        type: 'fill-item',
        itemId: match.encrypted.id,
        tabId: response.currentTab?.id,
      });
      $('status').textContent = result.ok ? 'Credentials filled on the page.' : result.error;
    });
    wrapper.appendChild(fill);
    matchesRoot.appendChild(wrapper);
  }

  if (response.pendingSave) {
    $('pending-title').value = response.pendingSave.title || response.currentTab?.title || '';
    $('pending-username').value = response.pendingSave.username || '';
    $('pending-email').value = response.pendingSave.email || '';
    $('pending-password').value = response.pendingSave.password || '';
  }
}

$('save-server').addEventListener('click', async () => {
  const result = await callBackground({
    type: 'set-server-url',
    serverBaseUrl: $('server-url').value,
  });
  $('status').textContent = result.ok ? 'Rustyfin server URL saved.' : result.error;
  await render();
});

$('pair-device').addEventListener('click', async () => {
  const result = await callBackground({
    type: 'pair-device',
    pairingCode: $('pairing-code').value,
    deviceName: 'Rustyfin Browser Extension',
  });
  $('status').textContent = result.ok ? 'Extension paired. Unlock it with the vault master password.' : result.error;
  await render();
});

$('unlock-vault').addEventListener('click', async () => {
  const result = await callBackground({
    type: 'unlock-vault',
    masterPassword: $('master-password').value,
  });
  $('status').textContent = result.ok ? 'Vault unlocked in extension memory.' : result.error;
  if (result.ok) {
    $('master-password').value = '';
  }
  await render();
});

$('lock-vault').addEventListener('click', async () => {
  await callBackground({ type: 'lock-vault' });
  $('status').textContent = 'Vault locked.';
  await render();
});

$('save-pending').addEventListener('click', async () => {
  const state = await callBackground({ type: 'get-popup-state' });
  const result = await callBackground({
    type: 'save-pending-item',
    tabId: state.currentTab?.id,
    title: $('pending-title').value,
    username: $('pending-username').value,
    email: $('pending-email').value,
    password: $('pending-password').value,
  });
  $('status').textContent = result.ok ? 'Captured login saved to Rustyfin Vault.' : result.error;
  await render();
});

render().catch((error) => {
  $('status').textContent = error instanceof Error ? error.message : String(error);
});
