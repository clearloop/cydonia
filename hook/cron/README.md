# walrus-cron

Cron job scheduling for Walrus agents.

Provides `CronJob`, `CronHandler` (implements `Hook`) for scheduling
periodic agent tasks. Requires an `on_create` callback at construction
for event loop integration.

## License

GPL-3.0
