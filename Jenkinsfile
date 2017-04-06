def build = fileLoader.fromGit('build', 'git@bitbucket.org:360dialog-berlin/jenkins-scripts.git', 'master', 'git', '')

node('master') {
  build.start { ->
    stage 'Checkout'
    checkout scm
    stage 'Submodule update'
    sh "git submodule update --init"
    stage "Create the binary"
    sh "cargo build --release"

    job_name = env.JOB_NAME.replaceFirst('.+/', '')

    if (job_name == "master") {
      stage "Upload binary to repository"
      sh "STAGE=production make upload"
      stage "Deployment"
      build.deploy("STAGE=production make auto_update")
    } else if (job_name == "develop") {
      stage "Upload binary to repository"
      sh "make upload"
      stage "Deployment"
      build.deploy("make auto_update")
    }
  }
}

