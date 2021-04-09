import json
from blaseball_mike.models import Team, Player
from dateparser import parse as dateparse
from unpaddedbase64 import encode_base64

def b64encode(s):
    return encode_base64(str(s).encode("utf8"),urlsafe=True) # pain

def process_event(event, player_cache, team_cache):
    id = b64encode(event['id'])
    event['id'] = id

    event['playerNames'] = []
    for player_id in event['playerTags']:
        name = player_cache.get(player_id,None)# we keep a player cache in memory to not have to reload it all every event
        if name != None:
            event['playerNames'].append(name) # it's in cache! so just use that
        else: # ok, it's not cached, let's load it up
            player = Player.load_one(player_id)

            if 'unscatteredName' in player.state: # is this player scattered? if so, get their unscattered name
                name = b64encode(player.state['unscatteredName'])
            else:
                name = b64encode(player.name) # trust me, i hate this as much as you do! but redisearch is tricky about characters in tag fields, so we have to

            player_cache[player_id] = name # append name to cache
            event['playerNames'].append(name) # append name to event tags

    event['teamNames'] = []
    for team_id in event['teamTags']:
        name = team_cache.get(team_id,None)
        if name != None: # team name's in cache, use that
            event['teamNames'].append(name)
        else:
            team = Team.load(team_id)
            name = b64encode(team.full_name)
            team_cache[team_id] = name
            event['teamNames'].append(name)

    # join tag lists into tag strings
    event['playerNames'] = '|'.join(event['playerNames'])
    event['teamNames'] = '|'.join(event['teamNames'])
    event['playerTags'] = '|'.join(map(b64encode,event['playerTags']))
    event['teamTags'] = '|'.join(map(b64encode,event['teamTags']))
    event['gameTags'] = '|'.join(map(b64encode,event['gameTags']))

    # flatten metadata into a single (cursed) array
    flattened_meta = []
    if 'metadata' in event and event['metadata'] != None:
        for k,v in event['metadata'].items():
            if type(v) == list:
                for aa in v: # it's an array; append a tag for each element
                    flattened_meta.append(b64encode(f'{k}?{aa}'))
            elif type(v) == dict:
                for ak, av in v.items():
                    flattened_meta.append(b64encode(f'{k}?{ak}?{av}')) # it's a dict; append a tag for each k:v pair
            else:
                flattened_meta.append(b64encode(f'{k}?{v}')) # it's just a normal field; append a tag for it

    event['etype'] = event['type']
    event.pop('type')
    event['metadata'] = '|'.join(flattened_meta)
    event['etimestamp'] = int(dateparse(event['created']).timestamp())
    event.pop('created')
    return event
