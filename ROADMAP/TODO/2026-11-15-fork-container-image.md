# Container images under the fork's own name

This checkout is `TommasoAmici/liwan`, a fork of `explodingcamera/liwan`. The
container workflow names the image after whoever pushed the tag and downloads the
binaries from the upstream repository, so a release cut here cannot publish
`ghcr.io/tommasoamici/liwan`: the name follows the pusher rather than the
repository owner, and the image would be assembled from upstream's tarballs
instead of this repository's release. The fork has never published an image.

## Checklist

- [ ] Name the image after the repository owner. In
      `.github/workflows/container.yaml`, change the metadata step's image:

  ```yaml
  images: ghcr.io/${{ github.repository_owner }}/liwan
  ```

  `docker/metadata-action` lowercases image names, so `TommasoAmici` becomes
  `tommasoamici`, which GHCR requires; do not add a separate lowercasing step.
  Keep the tag list as it is: `{{version}}`, `{{major}}.{{minor}}`, `{{major}}`,
  `edge`, and the `latest` that metadata-action adds for a semver tag. Do not
  hardcode `tommasoamici`: `github.repository_owner` is that value in this fork,
  and it stays correct for upstream if this change is ever proposed there. Leave
  the login step alone; `GITHUB_TOKEN` with `packages: write` is the right
  credential for a package in the repository owner's namespace.

- [ ] Package this repository's release binaries. The two `curl` lines in the
      same file fetch the x86_64 and aarch64 musl tarballs from
      `https://github.com/explodingcamera/liwan/releases/download/...`; change
      the repository part of both to `${{ github.repository }}`:

  ```yaml
  curl -fsSL "https://github.com/${{ github.repository }}/releases/download/$SAFE_TAG/liwan-x86_64-unknown-linux-musl.tar.gz" \
  ```

  `release.yaml` creates the release and uploads those tarballs in whichever
  repository runs it, so a tag pushed in this fork has its assets under
  `TommasoAmici/liwan`. With the fixed upstream URL a fork-only version would
  fail to download, and a version that exists in both repositories would
  silently package upstream's binary instead of this checkout. Keep `SAFE_TAG`
  and `-fsSL` as they are. After this change `.github/workflows/` mentions
  `explodingcamera` nowhere.

- [ ] Publish and verify one release. This part is owner-run; the change itself
      does not push a tag. The fork shows no workflow runs today, so check that
      Actions are enabled in its settings first. Push a `liwan-v*` tag in the
      fork, let the `Release` workflow publish the release, the binaries and the
      image, then pull `ghcr.io/tommasoamici/liwan:<version>` and `:latest` and
      check that the image reports the fork's version. A new GHCR package is
      private by default: make it public in the package settings if anonymous
      pulls are wanted. If the push is refused while a `liwan` package already
      exists under the account, link that package to this repository and rerun.

## Implementation context

**Decisions (2026-11-15).**

- The image belongs to the repository that builds it, so its namespace is
  `github.repository_owner`. The old `github.actor` was whoever triggered the
  run, so the name changed with the pusher and a tag pushed by a collaborator or
  bot targeted a namespace that account cannot write to.
- The binaries come from this fork's own release, as chosen by the owner. The
  image then always matches the commit that was tagged here. The cost is that a
  `liwan-v*` tag must be pushed in `TommasoAmici/liwan`; the fork has none yet,
  and the manual `container.yaml` dispatch only works for a tag that already has
  a release in this repository.
- Publishing stays release-tag-driven, as it is now. No image on a push to
  `main` and no image build on pull requests. `edge` keeps its current meaning:
  it is rewritten at each release, not on each commit to `main`.
- `README.md` is unchanged. Its container badge and install links describe the
  upstream project and its published image, which stay valid; hardcoding this
  account there would be wrong for upstream and would conflict on every rebase.
  Advertising this fork's package in its README is a separate, deliberate
  fork-only edit, not part of this plan.
- The change touches no code, database, config key, API shape, client package or
  dependency. It is two lines in one workflow file.

**Landmarks.**

- `.github/workflows/container.yaml`: the metadata step around line 41, the
  login step around lines 47-52, the download step around lines 53-66, and the
  build step around lines 67-75. `runs-on: ubuntu-26.04`, platforms
  `linux/amd64,linux/arm64`.
- `.github/workflows/release.yaml`: triggers on `liwan-v[0-9]+.*`; its
  `publish-container` job calls `$/.github/workflows/container.yaml` with
  `tag: ${{ github.ref_name }}` and passes `packages: write`.
- `data/Containerfile` consumes the prebuilt `dist/${TARGETARCH}/liwan`; the
  download step is the only thing that fills `dist/`, so the source of those
  files is the whole question this plan answers.
- The fork today: `main` is identical to upstream's, it has no tags, no
  releases, and no workflow runs.

**How to prove it.**

- Review the diff: exactly the image line and the two URLs change, and
  `grep -r explodingcamera .github` returns nothing.
- The `Release` run on the owner's first fork tag is the real test; it exercises
  the reusable workflow, the release assets and the push together. Pull both the
  version tag and `latest` afterwards, and confirm the package's linked
  repository is `TommasoAmici/liwan`.

**What the worker may decide.** Step names, comments and ordering inside the
workflow. It may not change the namespace strategy, the source of the binaries,
the tag list, the trigger, or `README.md`.

**Out of scope.** Images on `main` or pull requests, building the binary inside
the container, the self-hosted registry approach used in `clap-studio`
(`vars.REGISTRY` with `REGISTRY_USERNAME`/`REGISTRY_PASSWORD`), a new tag scheme,
`liwan.dev` documentation, and the npm and crate client packages, which have
their own workflows and namespaces.
