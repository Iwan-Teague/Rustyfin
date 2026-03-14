function isVisible(element) {
  if (!(element instanceof HTMLElement)) return false;
  const style = window.getComputedStyle(element);
  if (style.display === 'none' || style.visibility === 'hidden') return false;
  const rect = element.getBoundingClientRect();
  return rect.width > 0 && rect.height > 0;
}

function safeSendMessage(payload) {
  try {
    chrome.runtime.sendMessage(payload);
  } catch {
    // The extension context can disappear during navigation; ignore best-effort updates.
  }
}

function locateUsernameField(form) {
  const candidates = form.querySelectorAll(
    'input[type="text"], input[type="email"], input[name*="user" i], input[name*="email" i], input[autocomplete="username"], input[autocomplete="email"]',
  );
  return [...candidates].find((input) => isVisible(input)) || null;
}

function locatePasswordField(form) {
  const candidates = form.querySelectorAll('input[type="password"]');
  return [...candidates].find((input) => isVisible(input)) || null;
}

function notifyPageContext() {
  const hasPasswordField = Boolean(document.querySelector('input[type="password"]'));
  safeSendMessage({
    type: 'page-context',
    url: window.location.href,
    hasPasswordField,
  });
}

function captureFormSubmission(form) {
  const passwordField = locatePasswordField(form);
  if (!passwordField || !passwordField.value) {
    return;
  }
  const usernameField = locateUsernameField(form);
  safeSendMessage({
    type: 'credential-capture',
    payload: {
      title: document.title || window.location.hostname,
      url: window.location.href,
      username: usernameField?.value || '',
      email: usernameField?.type === 'email' ? usernameField.value : '',
      password: passwordField.value,
    },
  });
}

document.addEventListener(
  'submit',
  (event) => {
    const form = event.target instanceof HTMLFormElement ? event.target : null;
    if (form) {
      captureFormSubmission(form);
    }
  },
  true,
);

document.addEventListener('focusin', (event) => {
  const target = event.target;
  if (target instanceof HTMLInputElement && (target.type === 'password' || target.autocomplete === 'username')) {
    notifyPageContext();
  }
});

const mutationObserver = new MutationObserver(() => {
  if (document.querySelector('input[type="password"]')) {
    notifyPageContext();
  }
});

mutationObserver.observe(document.documentElement, {
  childList: true,
  subtree: true,
});

chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
  if (message.type === 'fill-credentials') {
    const passwordFields = [...document.querySelectorAll('input[type="password"]')].filter(isVisible);
    const visibleForms = new Set(
      passwordFields.map((field) => field.closest('form')).filter(Boolean),
    );
    const targetForm = visibleForms.values().next().value || document.querySelector('form');
    const usernameField = targetForm ? locateUsernameField(targetForm) : null;
    const passwordField = targetForm ? locatePasswordField(targetForm) : passwordFields[0] || null;

    if (usernameField) {
      usernameField.focus();
      usernameField.value = message.payload.username || message.payload.email || '';
      usernameField.dispatchEvent(new Event('input', { bubbles: true }));
      usernameField.dispatchEvent(new Event('change', { bubbles: true }));
    }
    if (passwordField) {
      passwordField.focus();
      passwordField.value = message.payload.password || '';
      passwordField.dispatchEvent(new Event('input', { bubbles: true }));
      passwordField.dispatchEvent(new Event('change', { bubbles: true }));
    }
    sendResponse({ ok: true });
    return true;
  }
  return false;
});

notifyPageContext();
