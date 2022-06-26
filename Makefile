GIT_LFS_CONFIG=git config --file .lfsconfig --replace-all

build:
	cargo build \
		--release \
		--out-dir ./bin \
		-Z unstable-options

# see https://github.com/sinbad/lfs-folderstore
config:
	$(GIT_LFS_CONFIG) lfs.customtransfer.skynet.path ./bin/git-lfs-web3
	$(GIT_LFS_CONFIG) lfs.customtransfer.skynet.args "transfer"
	$(GIT_LFS_CONFIG) lfs.customtransfer.skynet.concurrent false
	$(GIT_LFS_CONFIG) lfs.concurrenttransfers 2
	$(GIT_LFS_CONFIG) lfs.standalonetransferagent skynet
	$(GIT_LFS_CONFIG) lfs.transfer.maxretries 8