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

skynet-upload-test:
	curl \
		-L \
		-H "Skynet-Api-Key: UO93I5DTT1B1VR8MR9DNEOHFFTKATDQ990FVB6GI8C0SMJDFER9G" \
		-X POST \
		"https://skynetfree.net/skynet/skyfile/67da1154a858aa2d89f5201246f6d4b43a0a4eb55011136e993728f0daf8703e" \
		-F 'file=@/Users/luukdewaalmalefijt/Code/Partage/.git/lfs/objects/67/da/67da1154a858aa2d89f5201246f6d4b43a0a4eb55011136e993728f0daf8703e'