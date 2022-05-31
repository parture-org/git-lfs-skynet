# git-lfs-skynet

A [git-lfs](https://git-lfs.github.com/) custom transfer & extension that makes it easy to store large files on Sia Skynet.

## Installation

### Building

```bash
git clone https://github.com/parture-org/git-lfs-skynet
cd git-lfs-ipfs && make
```

### Packages

None yet!

### Configuration

If you haven't already, do `git lfs install` to set up Git LFS on your computer.

Add the custom transfer and extensions for skynet to your `./.git/config`:

```
make config
```

**Note that git-lfs-skynet will be enabled by default for all future LFS usage if you add these lines to your configuration. Make sure to remove them if you do not wish to do so.**

## Usage

Set ```SKYNET_API_KEY``` environment variable.

Use git LFS like you usually do and all subsequent files added in LFS will be added to your skynet portal.