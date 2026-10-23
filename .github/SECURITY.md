# Security policy

EUC Studio works on the firmware of electric unicycles, so a security problem here can also be a safety problem: a
behavior that could hurt a rider.

## Report a problem privately

Use GitHub's private vulnerability reporting:
[Report a vulnerability](https://github.com/tarikbc/euc-studio/security/advisories/new) (the repository's Security
tab). Do not open a public issue.

Report privately, for example:

- firmware behavior, stock as simulated or ours, that could make a wheel cut out or misbehave under a rider;
- a way for the flasher or the BLE layer to write to a real wheel past its gates;
- a way for vendor firmware material to pass the vendor guard.

Include the scenario or the steps, the commit (`git rev-parse --short HEAD`), the firmware build, and what you
expected. The safety rules are in
[SAFETY.md](https://github.com/tarikbc/euc-studio/blob/main/SAFETY.md#reporting-a-safety-problem).

## Supported versions

There is no release yet. Reports apply to the `main` branch.
