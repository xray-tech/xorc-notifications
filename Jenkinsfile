this.build = fileloader.fromgit('build', 'git@bitbucket.org:360dialog-berlin/jenkins-scripts.git', 'master', 'git', '')

node('master') {
  build.start { ->
    stage 'checkout'
    checkout scm
    stage 'submodule update'
    sh "git submodule update --init"
    stage "create the binary"
    sh "cargo build --release"

    job_name = env.job_name.replacefirst('.+/', '')

    if (job_name == "master") {
      stage "upload binary to repository"
      sh "stage=production make upload"
      stage "deployment"
      build.deploy("stage=production make auto_update")
    } else if (job_name == "develop") {
      stage "upload binary to repository"
      sh "make upload"
      stage "deployment"
      build.deploy("make auto_update")
    }
  }
}
