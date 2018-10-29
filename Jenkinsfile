def podConfig = """
apiVersion: v1
kind: Pod
metadata:
    name: dind
spec:
    serviceAccountName: jenkins
    containers:
      - name: docker
        image: docker/compose:1.21.2
        command: ['cat']
        tty: true
        env:
          - name: DOCKER_HOST
            value: tcp://localhost:2375
        volumeMounts:
          - name: cargo
            mountPath: /root/.cargo
          - name: cargo
            mountPath: /usr/src/xorc-notifications/target
          - name: service-account
            mountPath: /etc/service-account
      - name: dind-daemon
        image: docker:18.03.1-dind
        securityContext:
            privileged: true
        volumeMounts:
          - name: docker-graph-storage
            mountPath: /var/lib/docker
        resources:
          requests:
            cpu: 1
      - name: helm
        image: lachlanevenson/k8s-helm:v2.10.0
        command: ['cat']
        tty: true
    volumes:
      - name: docker-graph-storage
        persistentVolumeClaim:
          claimName: ci-graph-storage
      - name: cargo
        persistentVolumeClaim:
          claimName: ci-cargo-storage
      - name: service-account
        secret:
          secretName: service-account
"""

def label = "xray-notifications-${UUID.randomUUID().toString()}"

podTemplate(name: 'docker', label: label, yaml: podConfig) {
    node(label) {
        def scmVars = checkout([
            $class: 'GitSCM',
            branches: scm.branches,
            doGenerateSubmoduleConfigurations: false,
            extensions: [[
                $class: 'SubmoduleOption',
                disableSubmodules: false,
                parentCredentials: true,
                recursiveSubmodules: true,
                reference: '',
                trackingSubmodules: false
            ]],
            submoduleCfg: [],
            userRemoteConfigs: scm.userRemoteConfigs
        ])

        def gitCommit = scmVars.GIT_COMMIT

        container('docker') {
            stage('Build base image') {
                sh("docker login -u _json_key --password-stdin https://eu.gcr.io < /etc/service-account/xray2poc.json")
                def imageName = "eu.gcr.io/xray2poc/xorc-notifications"
                def image = "${imageName}:${gitCommit}"

                sh("docker build -t ${image} .")
                sh("docker push ${image}")
            }

            stage('Build consumer images') {
                parallel(
                    apns2: {
                        sh("docker login -u _json_key --password-stdin https://eu.gcr.io < /etc/service-account/xray2poc.json")
                        def image = "eu.gcr.io/xray2poc/apns2:${gitCommit}"
                        sh("docker build -t ${image} -f Dockerfile.apns2 --build-arg git_commit=$gitCommit .")
                        sh("docker push ${image}")
                    },
                    fcm: {
                        sh("docker login -u _json_key --password-stdin https://eu.gcr.io < /etc/service-account/xray2poc.json")
                        def image = "eu.gcr.io/xray2poc/fcm:${gitCommit}"
                        sh("docker build -t ${image} -f Dockerfile.fcm --build-arg git_commit=$gitCommit .")
                        sh("docker push ${image}")
                    },
                    web_push: {
                        sh("docker login -u _json_key --password-stdin https://eu.gcr.io < /etc/service-account/xray2poc.json")
                        def image = "eu.gcr.io/xray2poc/web_push:${gitCommit}"
                        sh("docker build -t ${image} -f Dockerfile.web_push --build-arg git_commit=$gitCommit .")
                        sh("docker push ${image}")
                    },
                    http: {
                        sh("docker login -u _json_key --password-stdin https://eu.gcr.io < /etc/service-account/xray2poc.json")
                        def image = "eu.gcr.io/xray2poc/http:${gitCommit}"
                        sh("docker build -t ${image} -f Dockerfile.http --build-arg git_commit=$gitCommit .")
                        sh("docker push ${image}")
                    }
                )
            }

            if (env.BRANCH_NAME == "master") {
                stage("Deploy"){
                    container('helm') {
                        parallel(
                            http: {
                                sh("helm upgrade -i --wait --set image.tag=${gitCommit} staging-http ./deploy/http -f deploy/staging.yaml")
                            },
                            apns: {
                                sh("helm upgrade -i --wait --set image.tag=${gitCommit} staging-apns2 ./deploy/apns2 -f deploy/staging.yaml")
                            },
                            fcm: {
                                sh("helm upgrade -i --wait --set image.tag=${gitCommit} staging-fcm ./deploy/fcm -f deploy/staging.yaml")
                            },
                            web_push: {
                                sh("helm upgrade -i --wait --set image.tag=${gitCommit} staging-web-push ./deploy/web-push -f deploy/staging.yaml")
                            }
                        )
                    }
                }
            }
        }
    }
}
