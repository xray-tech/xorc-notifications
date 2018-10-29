ARG git_commit
FROM eu.gcr.io/xray2poc/xorc-notifications:$git_commit
MAINTAINER Julius de Bruijn <julius@nauk.io>

RUN cp target/release/fcm /bin
RUN chmod a+x /bin/fcm

ENV CONFIG "/etc/xorc-notifications/fcm.toml"

WORKDIR /

CMD "/bin/fcm"
