# Security Policy

## Supported versions

Only the latest release of Playtime Tracker receives security fixes.

| Version | Supported |
| ------- | --------- |
| 2.x     | Yes       |
| < 2.0 (Game Session Tracker) | No; please update |

## Reporting a vulnerability

Please **don't open a public issue** for security problems. Instead, report them privately through
GitHub: go to this repository's **Security** tab → **Report a vulnerability**.

Include what you found, how to reproduce it, and which version you tested. You can expect an
acknowledgement within a week. If the report is confirmed, a fix is released as soon as practical and
you'll be credited in the release notes unless you'd rather not be.

## Scope and design notes

- The app runs entirely on your PC as your Windows user. It has no server, account or network
  service. Its only network requests are update checks to GitHub's API for this repository's latest
  release (can be turned off in Settings), and downloading that release's installer. An update is
  installed only if it comes from this repository's release over HTTPS and matches the size and
  SHA-256 checksum GitHub recorded for the file. (The *online* installer downloads the .NET runtime
  from Microsoft over HTTPS and only runs it if it is validly signed by Microsoft.)
- Because updates install automatically by default, access to this repository's releases is
  effectively access to every installed copy: protect the GitHub account with 2FA and the `v*` tags
  with a ruleset. Code-signing the installers would add a second, independent check.
- Play history and settings are stored with Windows DPAPI for the current user, which encrypts them
  and detects any change made outside the app. This prevents editing by hand; it is not designed to
  stop other software running as the same Windows user.
