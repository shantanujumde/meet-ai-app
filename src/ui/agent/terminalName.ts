/**
 * The name of the app the user types a sign-in command into: PowerShell on
 * Windows, Terminal elsewhere (TUR-53). The Rust side writes the command for
 * that shell (`agent_setup::view::sign_in_command`).
 *
 * Local until TUR-51's `src/lib/osText.ts` lands; then this moves there.
 */
export function terminalName(userAgent: string = navigator.userAgent): string {
  return /windows/i.test(userAgent) ? "PowerShell" : "Terminal";
}
