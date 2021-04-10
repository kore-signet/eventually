import aiohttp, aioredis, logging, asyncio, argparse, ujson
from utils import *

# cronch
class Monitor():
    def __init__(self,uri,delay):
        self.uri = uri
        self.delay = delay
        self.redis = None
        self.latest = None
        self.last_batch = set()
        self.player_cache = {}
        self.team_cache = {}
        self.session = None

    async def connect(self):
        self.redis = await aioredis.create_redis_pool(self.uri)
        self.session = aiohttp.ClientSession()

    async def insert(self,id,object):
        await self.redis.hmset_dict(f'event:{id}',object)

    async def poll(self):
        payload = {'limit': 100, 'sort': 1, 'start': self.latest} if self.latest else {'limit': 100, 'sort': 0}
        try:
            async with self.session.get('https://www.blaseball.com/database/feed/global',params=payload) as resp:
                events = await resp.json()

                events = [event for event in events if event['id'] not in self.last_batch] # have we already sent this event out? if yes, discard it
                self.last_batch = set([e['id'] for e in events]) # add this batch's event ids to the list of 'already done'

                event_amount = len(events)
                if event_amount > 0:
                    logging.info(f"Publishing {event_amount} new events!")

                    self.latest = events[-1]['created']
                    await self.redis.publish('feed',ujson.dumps({'data':events},ensure_ascii=False).encode('utf8'))

                    for e in events:
                        id = e['id']
                        logging.debug(f"Indexing event {id}")
                        processed = process_event(e,self.player_cache,self.team_cache)
                        await self.redis.hmset_dict(f"event:{id}",processed)
        except: # i'm sure this is Good Practice
            pass

    async def run(self):
        await self.connect()
        while True:
            await self.poll()
            await asyncio.sleep(self.delay)

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description='Consume the blaseball feed into a redis database')

    parser.add_argument('--url',dest='url',default='redis://localhost',help='Redis database url to connect to; defaults to localhost')
    parser.add_argument('--delay',dest='delay',type=int,default='1',help='How often (in seconds) to poll the feed; defaults to 1 second')
    parser.add_argument("--log-level", dest="logLevel", default='INFO', choices=['DEBUG', 'INFO', 'WARNING', 'ERROR', 'CRITICAL'], help="Set the logging level")

    args = parser.parse_args()
    logging.basicConfig(level=logging.getLevelName(args.logLevel))

    cronchy = Monitor(args.url,args.delay)
    logging.info("Starting the Feed Monitor!")
    asyncio.run(cronchy.run())
