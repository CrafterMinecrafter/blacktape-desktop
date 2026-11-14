// The middle window button: "Maximize" on a normal window, "Restore" on a maximized one.
export function maximizeButtonLabel(maximized: boolean): string {
  return maximized ? "Restore" : "Maximize";
}
