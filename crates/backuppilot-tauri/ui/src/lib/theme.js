export function applyColorScheme(scheme) {
    let resolved = scheme;
    if (scheme === 'system') {
        resolved = window.matchMedia('(prefers-color-scheme: dark)').matches ? 'dark' : 'light';
    }
    document.documentElement.setAttribute('data-theme', resolved);
}
