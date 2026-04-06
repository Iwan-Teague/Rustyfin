"use strict";
let activeField = null;
let overlayRoot = null;
let savePromptRoot = null;
function isVisible(element) {
    if (!(element instanceof HTMLElement))
        return false;
    const style = window.getComputedStyle(element);
    if (style.display === 'none' || style.visibility === 'hidden')
        return false;
    const rect = element.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
}
function safeSendMessage(payload) {
    try {
        return chrome.runtime.sendMessage(payload);
    }
    catch {
        return Promise.resolve({ ok: false, error: 'Extension context unavailable' });
    }
}
function fieldLabel(input) {
    const labelId = input.getAttribute('id');
    if (labelId) {
        const explicit = document.querySelector(`label[for="${CSS.escape(labelId)}"]`);
        if (explicit?.textContent?.trim()) {
            return explicit.textContent.trim().toLowerCase();
        }
    }
    const implicit = input.closest('label');
    if (implicit?.textContent?.trim()) {
        return implicit.textContent.trim().toLowerCase();
    }
    return '';
}
function fieldHint(input) {
    return [
        input.name,
        input.id,
        input.placeholder,
        input.autocomplete,
        input.getAttribute('aria-label'),
        fieldLabel(input),
    ]
        .filter(Boolean)
        .join(' ')
        .toLowerCase();
}
function pageKeywords() {
    return `${document.title} ${window.location.pathname}`.toLowerCase();
}
function topLevelUrl() {
    try {
        return window.top?.location?.href || window.location.href;
    }
    catch {
        return document.referrer || window.location.href;
    }
}
function visibleInputs() {
    return [...document.querySelectorAll('input, textarea')].filter(isVisible);
}
function firstByHint(inputs, matcher) {
    return inputs.find((input) => matcher(fieldHint(input))) || null;
}
function detectContext() {
    const inputs = visibleInputs();
    const passwordFields = inputs.filter((input) => input instanceof HTMLInputElement && input.type === 'password');
    const usernameField = firstByHint(inputs, (hint) => /user|email|login|identifier|account/.test(hint)) ||
        inputs.find((input) => input instanceof HTMLInputElement &&
            ['text', 'email'].includes(input.type) &&
            isVisible(input)) ||
        null;
    const currentPassword = firstByHint(passwordFields, (hint) => /current|old|existing/.test(hint)) ||
        null;
    const newPassword = firstByHint(passwordFields, (hint) => /new|create|choose/.test(hint)) ||
        passwordFields[passwordFields.length > 1 ? 1 : 0] ||
        null;
    const confirmPassword = firstByHint(passwordFields, (hint) => /confirm|repeat|verify/.test(hint)) ||
        passwordFields[passwordFields.length > 2 ? 2 : 1] ||
        null;
    let pageKind = 'unknown';
    const keywords = pageKeywords();
    if (passwordFields.length >= 2 && (currentPassword || /change|reset/.test(keywords))) {
        pageKind = 'change_password';
    }
    else if (passwordFields.length >= 1 && /sign.?up|register|create account|join/.test(keywords)) {
        pageKind = 'signup';
    }
    else if (passwordFields.length >= 1 && usernameField) {
        pageKind = 'login';
    }
    else if (passwordFields.length === 0 && usernameField) {
        pageKind = 'username_step';
    }
    return {
        pageKind,
        usernameField,
        passwordFields,
        currentPassword,
        newPassword,
        confirmPassword,
    };
}
function positionFloating(element, anchor) {
    const rect = anchor.getBoundingClientRect();
    const top = window.scrollY + rect.bottom + 8;
    const left = window.scrollX + rect.left;
    element.style.top = `${top}px`;
    element.style.left = `${left}px`;
    element.style.minWidth = `${Math.max(rect.width, 220)}px`;
}
function ensureOverlayRoot() {
    if (overlayRoot)
        return overlayRoot;
    overlayRoot = document.createElement('div');
    overlayRoot.style.position = 'absolute';
    overlayRoot.style.zIndex = '2147483647';
    overlayRoot.style.fontFamily = '"Avenir Next", "Segoe UI", sans-serif';
    overlayRoot.style.display = 'none';
    const shadow = overlayRoot.attachShadow({ mode: 'open' });
    const style = document.createElement('style');
    style.textContent = `
    .panel {
      border-radius: 16px;
      background: rgba(24, 29, 42, 0.96);
      border: 1px solid rgba(215, 223, 255, 0.14);
      box-shadow: 0 24px 60px rgba(0, 0, 0, 0.34);
      color: #f8f8ff;
      min-width: 240px;
      overflow: hidden;
    }
    .header {
      padding: 10px 12px 8px;
      font-size: 11px;
      letter-spacing: 0.18em;
      text-transform: uppercase;
      color: #ffc27a;
    }
    .list { display: flex; flex-direction: column; }
    .item, .action {
      all: unset;
      display: flex;
      flex-direction: column;
      gap: 2px;
      padding: 10px 12px;
      cursor: pointer;
      border-top: 1px solid rgba(255,255,255,0.06);
    }
    .item:hover, .action:hover {
      background: rgba(255,255,255,0.06);
    }
    .title { font-size: 13px; font-weight: 600; }
    .meta { font-size: 12px; color: #c4c9e1; }
  `;
    const mount = document.createElement('div');
    mount.className = 'panel';
    shadow.append(style, mount);
    document.documentElement.appendChild(overlayRoot);
    return overlayRoot;
}
function hideOverlay() {
    if (overlayRoot) {
        overlayRoot.style.display = 'none';
    }
}
async function showOverlayForField(field) {
    activeField = field;
    const response = await safeSendMessage({
        type: 'get-inline-state',
        tabId: undefined,
        url: window.location.href,
    });
    if (!response?.ok) {
        hideOverlay();
        return;
    }
    const state = response;
    if (!state.sitePermissionGranted || !state.unlocked || !state.settings.inlineAutofillEnabled) {
        hideOverlay();
        return;
    }
    const context = detectContext();
    const isPasswordField = field instanceof HTMLInputElement && field.type === 'password';
    const canGenerate = isPasswordField && ['signup', 'change_password'].includes(context.pageKind);
    if ((!state.matches || state.matches.length === 0) && !canGenerate) {
        hideOverlay();
        return;
    }
    const root = ensureOverlayRoot();
    const mount = root.shadowRoot.querySelector('.panel');
    mount.innerHTML = '';
    const header = document.createElement('div');
    header.className = 'header';
    header.textContent = 'RustyVault';
    mount.appendChild(header);
    const list = document.createElement('div');
    list.className = 'list';
    for (const match of state.matches || []) {
        const button = document.createElement('button');
        button.type = 'button';
        button.className = 'item';
        button.innerHTML = `<span class="title">${match.summary.title}</span><span class="meta">${match.summary.username || match.summary.login_email || match.summary.primary_uri || 'Saved login'}</span>`;
        button.addEventListener('mousedown', (event) => event.preventDefault());
        button.addEventListener('click', async () => {
            const result = await safeSendMessage({
                type: 'fill-item',
                itemId: match.encrypted.id,
                tabId: undefined,
            });
            if (result?.ok) {
                hideOverlay();
            }
        });
        list.appendChild(button);
    }
    if (canGenerate) {
        const action = document.createElement('button');
        action.type = 'button';
        action.className = 'action';
        action.innerHTML = `<span class="title">Generate password</span><span class="meta">Use your saved RustyVault generator defaults</span>`;
        action.addEventListener('mousedown', (event) => event.preventDefault());
        action.addEventListener('click', async () => {
            const result = await safeSendMessage({
                type: 'generate-password',
                tabId: undefined,
                url: window.location.href,
                pageKind: context.pageKind,
            });
            if (result?.ok && result.password) {
                fillGeneratedPassword(result.password);
                hideOverlay();
            }
        });
        list.appendChild(action);
    }
    mount.appendChild(list);
    root.style.display = 'block';
    positionFloating(root, field);
}
function fillField(input, value) {
    if (!input)
        return;
    input.focus();
    input.value = value;
    input.dispatchEvent(new Event('input', { bubbles: true }));
    input.dispatchEvent(new Event('change', { bubbles: true }));
}
function fillGeneratedPassword(password) {
    const context = detectContext();
    fillField(context.newPassword || context.passwordFields[0] || null, password);
    if (context.confirmPassword && context.confirmPassword !== context.newPassword) {
        fillField(context.confirmPassword, password);
    }
}
function createSavePrompt() {
    if (savePromptRoot)
        return savePromptRoot;
    savePromptRoot = document.createElement('div');
    savePromptRoot.style.position = 'fixed';
    savePromptRoot.style.right = '20px';
    savePromptRoot.style.bottom = '20px';
    savePromptRoot.style.zIndex = '2147483647';
    const shadow = savePromptRoot.attachShadow({ mode: 'open' });
    const style = document.createElement('style');
    style.textContent = `
    .prompt {
      inline-size: min(320px, calc(100vw - 32px));
      border-radius: 18px;
      background: rgba(24, 29, 42, 0.96);
      border: 1px solid rgba(215, 223, 255, 0.14);
      box-shadow: 0 24px 60px rgba(0, 0, 0, 0.34);
      color: #f8f8ff;
      padding: 14px;
      font-family: "Avenir Next", "Segoe UI", sans-serif;
    }
    .title { font-size: 11px; letter-spacing: 0.18em; text-transform: uppercase; color: #ffc27a; margin-bottom: 8px; }
    .message { font-size: 13px; color: #f8f8ff; margin-bottom: 12px; }
    .actions { display: flex; gap: 8px; }
    button {
      all: unset;
      border-radius: 999px;
      padding: 10px 14px;
      font-size: 13px;
      font-weight: 600;
      cursor: pointer;
    }
    .primary { background: linear-gradient(90deg, #ff914d 0%, #ff7588 54%, #9d74ff 100%); color: white; }
    .secondary { background: rgba(255,255,255,0.08); color: white; }
  `;
    const mount = document.createElement('div');
    shadow.append(style, mount);
    document.documentElement.appendChild(savePromptRoot);
    return savePromptRoot;
}
function hideSavePrompt() {
    if (savePromptRoot) {
        savePromptRoot.remove();
        savePromptRoot = null;
    }
}
async function showSavePrompt(kind, message) {
    const root = createSavePrompt();
    const mount = root.shadowRoot.querySelector('div');
    mount.className = 'prompt';
    mount.innerHTML = '';
    const title = document.createElement('div');
    title.className = 'title';
    title.textContent =
        kind === 'update_existing' ? 'Update login' : kind === 'add_uri' ? 'Add site' : 'Save login';
    const body = document.createElement('div');
    body.className = 'message';
    body.textContent = message;
    const actions = document.createElement('div');
    actions.className = 'actions';
    const confirm = document.createElement('button');
    confirm.className = 'primary';
    confirm.textContent = kind === 'update_existing' ? 'Update' : kind === 'add_uri' ? 'Add site' : 'Save';
    confirm.addEventListener('click', async () => {
        await safeSendMessage({ type: 'save-pending-item', tabId: undefined });
        hideSavePrompt();
    });
    const dismiss = document.createElement('button');
    dismiss.className = 'secondary';
    dismiss.textContent = 'Dismiss';
    dismiss.addEventListener('click', async () => {
        await safeSendMessage({ type: 'dismiss-pending-item', tabId: undefined });
        hideSavePrompt();
    });
    actions.append(confirm, dismiss);
    mount.append(title, body, actions);
}
function notifyPageContext() {
    const context = detectContext();
    safeSendMessage({
        type: 'page-context',
        payload: {
            url: window.location.href,
            topLevelUrl: window.top === window ? window.location.href : topLevelUrl(),
            isTopFrame: window.top === window,
            hasPasswordField: context.passwordFields.length > 0,
            pageKind: context.pageKind,
        },
    });
}
function captureCredentialAttempt() {
    const context = detectContext();
    const usernameValue = (context.usernameField instanceof HTMLInputElement || context.usernameField instanceof HTMLTextAreaElement)
        ? context.usernameField.value
        : '';
    const emailValue = context.usernameField instanceof HTMLInputElement && context.usernameField.type === 'email'
        ? context.usernameField.value
        : '';
    const passwordValue = context.pageKind === 'change_password'
        ? context.newPassword?.value || context.passwordFields[0]?.value || ''
        : context.passwordFields[0]?.value || '';
    if (!passwordValue) {
        return;
    }
    safeSendMessage({
        type: 'credential-capture',
        payload: {
            title: document.title || window.location.hostname,
            url: window.location.href,
            username: usernameValue,
            email: emailValue,
            password: passwordValue,
            pageKind: context.pageKind,
            pagePasswordCount: context.passwordFields.length,
        },
    });
}
function submitActionHint(target) {
    if (!target)
        return '';
    const button = target.closest('button, input[type="submit"], input[type="button"], input[type="image"]');
    if (!button || !isVisible(button)) {
        return '';
    }
    const inputValue = button instanceof HTMLInputElement ? button.value : '';
    return [
        button.textContent,
        button.getAttribute('aria-label'),
        button.getAttribute('name'),
        button.getAttribute('id'),
        inputValue,
    ]
        .filter(Boolean)
        .join(' ')
        .toLowerCase();
}
function isCredentialSubmitTrigger(target) {
    if (!(target instanceof HTMLElement)) {
        return false;
    }
    const hint = submitActionHint(target);
    if (!hint) {
        return false;
    }
    return /(sign.?in|log.?in|continue|next|submit|register|sign.?up|create account|join|save|update password|change password|reset password)/.test(hint);
}
document.addEventListener('submit', (event) => {
    const form = event.target instanceof HTMLFormElement ? event.target : null;
    if (form) {
        captureCredentialAttempt();
    }
}, true);
document.addEventListener('focusin', (event) => {
    const target = event.target;
    if (target instanceof HTMLInputElement &&
        (target.type === 'password' ||
            target.type === 'email' ||
            target.type === 'text' ||
            target.autocomplete === 'username')) {
        notifyPageContext();
        showOverlayForField(target).catch(() => null);
    }
});
document.addEventListener('click', (event) => {
    if (!overlayRoot)
        return;
    const target = event.target;
    if (!target)
        return;
    const shadow = overlayRoot.shadowRoot;
    if (overlayRoot.contains(target) || shadow?.contains(target)) {
        return;
    }
    hideOverlay();
});
document.addEventListener('click', (event) => {
    if (isCredentialSubmitTrigger(event.target)) {
        captureCredentialAttempt();
    }
}, true);
window.addEventListener('scroll', () => {
    if (overlayRoot && activeField && overlayRoot.style.display !== 'none') {
        positionFloating(overlayRoot, activeField);
    }
});
const mutationObserver = new MutationObserver(() => {
    notifyPageContext();
});
mutationObserver.observe(document.documentElement, {
    childList: true,
    subtree: true,
});
chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
    if (message.type === 'fill-credentials') {
        const context = detectContext();
        fillField(context.usernameField, message.payload.username || message.payload.email || '');
        fillField(context.passwordFields[0] || null, message.payload.password || '');
        sendResponse({ ok: true });
        return true;
    }
    if (message.type === 'show-save-prompt') {
        showSavePrompt(message.payload.kind, message.payload.message).catch(() => null);
        sendResponse({ ok: true });
        return true;
    }
    if (message.type === 'dismiss-save-prompt') {
        hideSavePrompt();
        sendResponse({ ok: true });
        return true;
    }
    if (message.type === 'generated-password') {
        fillGeneratedPassword(message.payload.password);
        sendResponse({ ok: true });
        return true;
    }
    return false;
});
notifyPageContext();
