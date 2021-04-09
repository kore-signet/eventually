from quart import Quart
from typing import AsyncGenerator
from aioredis import Channel
import aioredis, json


class EventuallyRedis:
    def __init__(self, app: Quart, address: str) -> None:
        self.init_app(app)
        self._pool = None
        self._uri = address

    def init_app(self, app: Quart) -> None:
        app.before_serving(self._before_serving)
        app.after_serving(self._after_serving)

    async def _before_serving(self) -> None:
        self._pool = await aioredis.create_redis_pool(self._uri)

    async def _after_serving(self) -> None:
        self._pool.close()
        await self._pool.wait_closed()

    async def subscribe(self) -> AsyncGenerator:
        (subscription,) = await self._pool.subscribe('feed')
        while await subscription.wait_message():
            yield json.dumps({'event':'message','data': await subscription.get_json()}).encode("utf8")

    async def run(self,*args):
        return await self._pool.execute(*args)
