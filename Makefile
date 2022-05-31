build:
	cd git-lfs-ipfs-cli && \
		cargo build \
			--release \
			--out-dir ../bin \
			-Z unstable-options

# see https://github.com/sinbad/lfs-folderstore
config:
	git config --replace-all lfs.customtransfer.skynet.path ${shell pwd}/bin/git-lfs-skynet
	git config --replace-all lfs.customtransfer.skynet.args "transfer"
	# todo: set to false and do internal chunking?
	git config --replace-all lfs.customtransfer.skynet.concurrent true
	git config --replace-all lfs.concurrenttransfers 2
	git config --replace-all lfs.standalonetransferagent skynet