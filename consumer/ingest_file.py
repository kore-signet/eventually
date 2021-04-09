import json
from blaseball_mike.models import Team, Player
import redis
from dateparser import parse as dateparse
from utils import *

r = redis.Redis()

player_cache = {}
team_cache = {}

with open("feed.json") as f:
    i = 0
    for line in f:
        event = json.loads(line)
        event = process_event(event,player_cache,team_cache)
        event['etype'] = event['type']
        del event['type']
        print(f'#{i} setting event:{event["id"]}' )
        r.hmset(f'event:{event["id"]}',event)
        i += 1
