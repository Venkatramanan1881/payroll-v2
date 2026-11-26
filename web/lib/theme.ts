/** Sets light/dark on <html data-theme> and remembers it for this browser. The palette is fixed in app/layout.tsx. */
export function setTheme(value: "dark" | "light") {
  document.documentElement.dataset.theme = value;
  try {
    localStorage.setItem("theme", value);
  } catch {}
}
