# src/worker.py
import asyncio
from datetime import timedelta
from temporalio import activity, workflow
from temporalio.client import Client
from temporalio.worker import Worker
import os

@activity.defn
async def my_activity(input: str) -> dict:
    """Activity - can have side effects"""
    activity.logger.info(f"Processing: {input}")
    return {"success": True, "message": f"Processed {input}"}


@workflow.defn
class MyWorkflow:
    @workflow.run
    async def run(self, data: str) -> str:
        result = await workflow.execute_activity(
            my_activity,
            data,
            start_to_close_timeout=timedelta(minutes=10),
            retry_policy=workflow.RetryPolicy(
                initial_interval=timedelta(seconds=30),
                backoff_coefficient=2,
                maximum_attempts=3,
            ),
        )
        if not result["success"]:
            raise Exception(f"Activity failed: {result['message']}")
        return result["message"]


async def main():
    client = await Client.connect(
        os.getenv("TEMPORAL_ADDRESS", "localhost:7233")
    )
    worker = Worker(
        client,
        task_queue=os.getenv("TASK_QUEUE", "my-tasks"),
        workflows=[MyWorkflow],
        activities=[my_activity],
    )
    print(f"Worker started on queue: {worker.task_queue}")
    await worker.run()


if __name__ == "__main__":
    asyncio.run(main())