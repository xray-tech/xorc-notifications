ARG git_commit
FROM eu.gcr.io/xray2poc/xorc-notifications:$git_commit
MAINTAINER Julius de Bruijn <julius@nauk.io>

RUN cp target/release/web_push /bin
RUN chmod a+x /bin/web_push

ENV CONFIG "/etc/xorc-notifications/web_push.toml"

WORKDIR /

CMD "/bin/web_push"
