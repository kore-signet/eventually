FROM debian:bullseye-slim

RUN apt-get update && apt-get -y dist-upgrade
RUN apt-get install -y python3 python3-pip --no-install-recommends

COPY monitor.py /root
COPY utils.py /root
COPY requirements.txt /root

WORKDIR /root
RUN python3 -m pip install -r requirements.txt

CMD ["python3","monitor.py","--url","redis://redis"]
