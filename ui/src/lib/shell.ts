// Command-line pieces built in the frontend.

/** POSIX single-quote escaping, as PHP's `escapeshellarg`. */
export function shellQuote(s: string): string {
  return `'${s.replace(/'/g, `'\\''`)}'`;
}

/** v1 `Ongoing::resumeCommandWithCd`: `cd '<path>' && claude '/ongoing-restart <name>'`. */
export function ongoingResumeCommand(projectPath: string, name: string): string {
  return `cd ${shellQuote(projectPath)} && claude ${shellQuote(`/ongoing-restart ${name}`)}`;
}
