function normalize(value) {
    return (value || '').trim().toLowerCase();
}
function sameIdentity(summary, draft) {
    const draftUser = normalize(draft.username);
    const draftEmail = normalize(draft.email);
    const summaryUser = normalize(summary.username || '');
    const summaryEmail = normalize(summary.login_email || '');
    return Boolean((draftUser && draftUser === summaryUser) ||
        (draftEmail && draftEmail === summaryEmail) ||
        (draftUser && draftUser === summaryEmail) ||
        (draftEmail && draftEmail === summaryUser));
}
function uriAlreadyPresent(item, url) {
    const current = normalize(url);
    return normalize(item?.summary.primary_uri || '') === current;
}
export function classifyPendingAction(args) {
    const { tabId, draft, matches, lastFilled, pageKind } = args;
    if (!draft.password.trim()) {
        return null;
    }
    const lastFilledMatch = lastFilled
        ? matches.find((match) => match.encrypted.id === lastFilled.itemId)
        : null;
    const identityMatch = matches.find((match) => sameIdentity(match.summary, draft));
    if (pageKind === 'change_password' && (lastFilledMatch || identityMatch)) {
        const target = lastFilledMatch || identityMatch;
        return {
            kind: 'update_existing',
            tabId,
            itemId: target.encrypted.id,
            message: `Update the saved password for ${target.summary.title || 'this login'}?`,
            draft,
            createdAt: Date.now(),
        };
    }
    if (identityMatch) {
        return {
            kind: 'update_existing',
            tabId,
            itemId: identityMatch.encrypted.id,
            message: `Update the saved login for ${identityMatch.summary.title || 'this account'}?`,
            draft,
            createdAt: Date.now(),
        };
    }
    if (lastFilled && !uriAlreadyPresent(lastFilledMatch, draft.url)) {
        return {
            kind: 'add_uri',
            tabId,
            itemId: lastFilled.itemId,
            message: `Allow ${(lastFilledMatch?.summary.title || draft.title || 'this login')} on ${new URL(draft.url).hostname}?`,
            draft,
            createdAt: Date.now(),
        };
    }
    return {
        kind: 'save_new',
        tabId,
        message: `Save a new login for ${new URL(draft.url).hostname}?`,
        draft,
        createdAt: Date.now(),
    };
}
