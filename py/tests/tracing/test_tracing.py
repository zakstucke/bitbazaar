def test_tracing():
    import logging
    import pathlib

    from fastapi import FastAPI
    from opentelemetry.instrumentation.fastapi import FastAPIInstrumentor

    from bitbazaar.tracing import GlobalLog

    # create your FastAPI app
    app = FastAPI()

    log = GlobalLog(
        {
            "service_name": "MA SERVICE",
            "console": {"from_level": logging.NOTSET},
            # "file": {
            #     "from_level": logging.NOTSET,
            #     "logpath": pathlib.Path("logs.log"),
            #     "max_backups": 5,
            #     "max_bytes": 1000000,
            # },
        }
    )

    # Hook up automatics for fastapi:
    FastAPIInstrumentor.instrument_app(app, tracer_provider=log.provider)

    @app.get("/")
    async def index():
        log.debug("I AM DEBUG!")
        log.info("I AM INFO!")
        log.warn("I AM WARN!", extra={"foo": "bar", "ree": [1, 2, 3]})
        log.error("I AM ERRROR!")
        log.crit("I AM CRIT\nbut I am very \nmultline\n\n\n\nI know its crazy!\n\n\n\n")
        return {"foo": "bar"}

    from fastapi.testclient import TestClient

    with TestClient(app) as client:
        client.get("/").json()
