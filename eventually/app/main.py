from quart import Quart, request
from unpaddedbase64 import encode_base64, decode_base64
from typing import Optional
from db import EventuallyRedis
from datetime import datetime
import json
import toml

def b64encode(s):
    return encode_base64(str(s).encode("utf8"),urlsafe=True) # pain

def b64decode(s):
    return decode_base64(s).decode("utf8")

# ids=[uuid]
# playerNames=[str]
# playerTags=[uuid]
# teamTags=[uuid]
# teamNames=[uuid]
# gameTags=[uuid]
# metadata.[field]=[values]
# metadata.[field].[subfield]=values is also legal
# type=[int]
# category=[int]
# tournament=int
# before=timestamp
# after=timestamp
# seasons=[int]
# phase_min=int
# phase_max=int
# day_min=int
# day_max=int
app = Quart(__name__)
app.config.from_file("eventually.toml",toml.load)
redis = EventuallyRedis(app,app.config['DATABASE_URL'])

def pairs(iterable):
    return zip(*[iter(iterable)]*2)

def try_int(s):
    try:
        val = int(s)
    except:
        val = s
    return val

tag_fields = ["ids","season","tournament","category","type","gameTags","teamNames","teamTags","playerTags","playerNames"]
base64_tag_fields = ["gameTags","teamNames","teamTags","playerTags","playerNames"]

def parse_event(res):
    event = {}
    for k,v in pairs(res):
        k,v = k.decode("utf8"), v.decode("utf8")
        if k in base64_tag_fields:
            event[k] = []
            for val in v.split('|'):
                event[k].append(b64decode(val))
        elif k == 'id':
            event[k] = b64decode(v)
        elif k == 'metadata':
            event['metadata'] = {}
            for val in v.split('|'):
                fields = b64decode(val).split('?')
                if len(fields) == 3:
                    if fields[0] not in event['metadata']:
                        event['metadata'][fields[0]] = {}
                    event['metadata'][fields[0]][fields[1]] = fields[2] # metadata field format = field.subfield.value, so this is basically event['metadata'][field][subfield] = value
                else:
                    if fields[0] not in event['metadata']:
                        event['metadata'][fields[0]] = []
                    event['metadata'][fields[0]].append(fields[1]) # metadata field = field.value, so this is is event['metadata'][field] = value. we use an array because we can't be sure if it's not one at this point in parsing lol

            # TODO: use a proper type schema instead of this
            for k,v in event['metadata'].items(): # do some niceties in parsing, like turning string fields into ints and turning the single-field arrays created in the first step into just fields
                if type(v) == list and len(v) == 1:
                    event['metadata'][k] = try_int(v[0])
                else:
                    event['metadata'][k] = try_int(v)
        else:
            event[k] = v

    event['created'] = datetime.utcfromtimestamp(int(event['etimestamp'])).isoformat()
    del event['etimestamp']
    event['type'] = event['etype']
    del event['etype']

    return event

@app.route('/events')
async def events():
    def format_tags(field_name,field_vals):
        tags = map(b64encode,field_vals)
        return f"@{field_name}:{{{'|'.join(tags)}}}"

    def format_time_range(field: str, before: Optional[int], after: Optional[int]):
        return f"@{field}:[{after if after else '-inf'} {before if before else 'inf'}]"

    def format_range(field: str, min: Optional[int], max: Optional[int]):
        return f"@{field}:[{min if min else '-inf'} {max if max else 'inf'}]"

    args = dict(request.args)

    before = args.pop('before', None)
    after = args.pop('after', None)
    phase_min = args.pop('phase_min', None)
    phase_max = args.pop('phase_max', None)
    day_min = args.pop('day_min', None)
    day_max = args.pop('day_max', None)

    query = []

    if before or after:
        query.append(format_time_range('etimestamp',before,after))

    if phase_min or phase_max:
        query.append(format_range('phase',phase_min,phase_max))

    if day_min or day_max:
        query.append(format_range('day',day_min,day_max))

    for k, v in args.items():
        if k in tag_fields:
            if k == 'type':
                query.append(format_tags('etype',v.split(',')))
            else:
                query.append(format_tags(k,v.split(',')))
        elif k.startswith('metadata'):
            field = k.split('.')[1:]
            values = v.split(',')
            for val in values:
                query.append(f"@metadata:{{{b64encode('?'.join(field + [val]))}}}")

    json_res = []
    res = await redis.run("FT.SEARCH", "eventIndex", " ".join(query))
    for _, e in pairs(res[1:]):
        json_res.append(parse_event(e))

    return json.dumps(json_res)

@app.route('/sse')
async def live_feed():
    return redis.subscribe(), {
        'Content-Type': 'text/event-stream',
        'Cache-Control': 'no-cache',
        'Transfer-Encoding': 'chunked'
    }

if __name__ == "__main__":
    app.run(debug=True)
