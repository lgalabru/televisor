"use client";

import { Button } from "@solana/design-system/button";
import { CopyButton } from "@solana/design-system/copy-button";

const installCommand =
  "git clone https://github.com/lgalabru/televisor && cd televisor && just install";

function ArrowIcon() {
  return (
    <svg aria-hidden="true" viewBox="0 0 20 20" fill="none">
      <path d="M4 10h11M11 6l4 4-4 4" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" />
    </svg>
  );
}

function GithubIcon() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" fill="currentColor">
      <path d="M12 2a10 10 0 0 0-3.16 19.49c.5.09.68-.22.68-.48v-1.87c-2.78.6-3.37-1.18-3.37-1.18-.45-1.16-1.11-1.47-1.11-1.47-.91-.62.07-.61.07-.61 1 .07 1.53 1.03 1.53 1.03.9 1.53 2.35 1.09 2.92.83.09-.65.35-1.09.64-1.34-2.22-.25-4.56-1.11-4.56-4.94 0-1.09.39-1.98 1.03-2.68-.1-.25-.45-1.27.1-2.64 0 0 .84-.27 2.75 1.02A9.58 9.58 0 0 1 12 6.82c.85 0 1.71.12 2.51.34 1.91-1.29 2.75-1.02 2.75-1.02.55 1.37.2 2.39.1 2.64.64.7 1.03 1.59 1.03 2.68 0 3.84-2.34 4.69-4.57 4.93.36.31.68.92.68 1.86v2.76c0 .27.18.58.69.48A10 10 0 0 0 12 2Z" />
    </svg>
  );
}

export function InstallActions() {
  return (
    <div className="hero-actions">
      <div className="install-command" aria-label={`Install command: ${installCommand}`}>
        <span aria-hidden="true" className="prompt-mark">$</span>
        <code>git clone github.com/lgalabru/televisor</code>
        <CopyButton
          value={installCommand}
          variant="inline"
          size="lg"
          label="Copy install"
          copiedLabel="Copied"
          className="copy-install"
        />
      </div>
      <Button
        asChild
        variant="secondary"
        size="lg"
        iconLeft={<GithubIcon />}
        iconRight={<ArrowIcon />}
        className="github-button"
      >
        <a href="https://github.com/lgalabru/televisor" target="_blank" rel="noreferrer">
          View on GitHub
        </a>
      </Button>
    </div>
  );
}
