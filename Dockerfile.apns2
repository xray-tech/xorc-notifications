ARG git_commit
FROM eu.gcr.io/xray2poc/xorc-notifications:$git_commit
MAINTAINER Julius de Bruijn <julius@nauk.io>

RUN cp target/release/apns2 /bin
RUN chmod a+x /bin/apns2

ENV CONFIG "/etc/xorc-notifications/apns2.toml"

WORKDIR /

CMD "/bin/apns2"
