# Code signing policy

Pulse's Windows releases (`pulse.exe` and the `PulseSetup` installer) are built from this public repository by GitHub Actions, never on a personal computer. The build workflow is [`.github/workflows/build.yml`](.github/workflows/build.yml), and every release links to the run that built it.

## Windows code signing

Application to the SignPath Foundation open-source program: submitted, pending review.

Once approved: free code signing provided by [SignPath.io](https://signpath.io), certificate by [SignPath Foundation](https://signpath.org).

## Build provenance

Starting with the next release, each build of `main` also records a GitHub-signed attestation that the files were built here from a specific commit. Anyone can check a download with:

```
gh attestation verify PulseSetup-X.Y.Z.exe -R mercy123xz/Pulse
```

## Team roles

- Committers and reviewers: [MERCY (@mercy123xz)](https://github.com/mercy123xz)
- Approvers: [MERCY (@mercy123xz)](https://github.com/mercy123xz)
- Contributors: [Lachlan (@lachlan1234564)](https://github.com/lachlan1234564)

Contributors send changes as pull requests. Every pull request is reviewed and merged by MERCY before it is built and signed; only code merged into this repository is built and signed.

## Privacy

Pulse connects to the internet only to do what it's for: searching and streaming music (Deezer for song information, YouTube for audio), lyrics (LRCLIB), and, if you choose to make one, your Pulse account. It has no ads and no tracking. What an account stores, and how to delete it, is described in the [privacy policy](https://listen2pulse.github.io/privacy/).
