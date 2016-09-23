node('master') {
  wrap([$class: 'AnsiColorBuildWrapper', 'colorMapName': 'XTerm']) {
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
      input "Ready to deploy?"
      sh "STAGE=production make auto_update"
    } else if (job_name == "develop") {
      stage "Upload binary to repository"
      sh "make upload"

      stage "Deployment"
      input "Ready to deploy?"
      sh "make auto_update"
    }
  }
}
