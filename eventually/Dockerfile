FROM tiangolo/uvicorn-gunicorn:python3.8-slim

COPY ./app /app
WORKDIR /app
RUN python3 -m pip install -r requirements.txt
WORKDIR /
