ARG git_commit
FROM eu.gcr.io/xray2poc/xorc-notifications:$git_commit
MAINTAINER Julius de Bruijn <julius@nauk.io>

RUN cp target/release/http_requester /bin
RUN chmod a+x /bin/http_requester

ENV CONFIG "/etc/xorc-notifications/http_requester.toml"

WORKDIR /

CMD "/bin/http_requester"
