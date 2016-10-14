def notify = fileLoader.fromGit('notify', 'git@bitbucket.org:360dialog-berlin/jenkins-scripts.git', 'master', 'git', '')

node('master') {
  wrap([$class: 'AnsiColorBuildWrapper', 'colorMapName': 'XTerm']) {
    try {
      notify.slack('Build started')

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
        notify.slack("Deployment pending, please confirm")
        input "Ready to deploy?"
        notify.slack("Deployment started")
        sh "STAGE=production make auto_update"
      } else if (job_name == "develop") {
        stage "Upload binary to repository"
        sh "make upload"

        stage "Deployment"
        notify.slack("Deployment pending, please confirm")
        input "Ready to deploy?"
        notify.slack("Deployment started")
        sh "make auto_update"
      }
    } catch (error) {
      currentBuild.result = 'FAILED'
      throw error
    } finally {
      notify.slack(currentBuild.result)
    }
  }
}
