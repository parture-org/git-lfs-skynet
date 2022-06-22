build:
	cargo build \
		--release \
		--out-dir ./bin \
		-Z unstable-options
	mv ./bin/git-lfs-web3 ../../bin/

# see https://github.com/sinbad/lfs-folderstore
config:
	git config --replace-all lfs.customtransfer.skynet.path ${shell pwd}/bin/git-lfs-web3
	git config --replace-all lfs.customtransfer.skynet.args "transfer"
	# todo: set to false and do internal chunking?
	git config --replace-all lfs.customtransfer.skynet.concurrent false
	git config --replace-all lfs.concurrenttransfers 2
	git config --replace-all lfs.standalonetransferagent skynet