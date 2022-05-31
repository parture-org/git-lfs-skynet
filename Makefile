build:
	cd git-lfs-ipfs-cli && \
		cargo build \
			--release \
			--out-dir ../../bin \
			-Z unstable-options
