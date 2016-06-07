SHELL = /bin/sh
executable = target/release/apns2
artifactory = http://repo:katti@artifactory.service.consul:8081/artifactory/apns2
commit_id = `git log --format="%h" -n 1`
marathon = http://leader.mesos.service.consul:8080/v2/apps/
influx = "http://influxdb.service.consul:8086/write?db=deployments"
config = deploy/config.mar.template
curl = `which curl`
deplicity = `which deplicity`

.PHONY: help

help:
	@grep -E '^[a-zA-Z_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-30s\033[0m %s\n", $$1, $$2}'

update: ## Update the running Mesos configuration
	$(deplicity) -i $(influx) -m $(marathon) -j $(config) -v $(commit_id) simple

auto_update: ## Update the running Mesos configuration, don't ask questions
	$(deplicity) -f -i $(influx) -m $(marathon) -j $(config) -v $(commit_id) simple

upload: ## Upload the assembly jar to the repository
	$(curl) -T $(executable) $(artifactory)/production/apns2-$(commit_id)

