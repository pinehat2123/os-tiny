# This is `Makefile`

.PHONY:  kernel run clean dir gitlab

dir:
	cd ${HOME}/Documents/whale/tiny
clean:
	bash ./script/clean
gitlab:
	bash ./script/quick-push
	
