node('master') {
  sh "git submodule update --init"

  stage "Create the binary"
  sh "cargo build --release"

  stage "Upload binary to repository"
  sh "make upload"

  stage "Deployment"
  input "Ready to deploy?"
  sh "make auto_update"
}


