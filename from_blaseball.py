import requests
import time
import uuid
from dateparser import parse as dateparse
import asyncpg
import json
import pprint

async def main():
    conn = await asyncpg.connect('postgresql://allie@localhost/eventually-dev')

    s = requests.Session()
    params = {"sort": 1, "limit": 200}

    while True:
        async with conn.transaction():
            r = s.get("https://www.blaseball.com/database/feed/global",params=params)
            ev = r.json()
    
            print(f"latest processed: {ev[-1]['id']}, {ev[-1]['description']}")
            print(f"got {len(ev)}")
            print(f"latest: {ev[-1]['created']}")
    
            params["start"] = ev[-1]['created']

            processed = []
            for event in ev:
                event["created"] = int(dateparse(event["created"]).timestamp())
                res = await conn.fetchrow("INSERT INTO documents (doc_id,object) VALUES ($1, $2) ON CONFLICT (doc_id) DO UPDATE SET object = $2", uuid.UUID(event['id']), json.dumps(event))
#                print(res)
#                if res == None:
#                    print(f"found missing: {res}")

        time.sleep(3)

import asyncio
asyncio.get_event_loop().run_until_complete(main())
