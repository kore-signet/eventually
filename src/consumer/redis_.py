#  CONSUMERS ATTACK CHORBY SOUL
class ConsumerRedis():
    def __init__(self,uri):
        self.uri = uri
        self.redis = None

    async def connect(self):
        self.redis = await aioredis.create_redis_pool('redis://localhost')

    async def insert(self,id,object):
        await self.redis.hmset_dict(f'event:{id}',object)

    def process_event(self,event):


https://www.blaseball.com/database/feed/global?=&limit=50&sort=0
