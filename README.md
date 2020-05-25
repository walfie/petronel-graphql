# petronel-graphql

## Development

```bash
# Secrets needed for connecting to the Twitter streaming API
export CONSUMER_KEY="..."
export CONSUMER_SECRET="..."
export ACCESS_TOKEN="..."
export ACCESS_TOKEN_SECRET="..."

cargo run
```

By default, it will start an HTTP server on port 8080, with a GraphiQL
interface available at <http://localhost:8080/graphiql>.

You can run `cargo run -- --help` to see some other config options.
Options can generally be configured via environment variables, or as
command-line arguments.

Some useful environment variables:

```bash
# By default, the HTTP server binds to 127.0.0.0:8080,
# but you can specify different values:
export BIND_IP=0.0.0.0
export BIND_PORT=9999

# Specify how many tweets (per boss) we should keep in history
export RAID_HISTORY_SIZE=50

# On startup, will attempt to load the list of bosses from a JSON file,
# and write to the file every 30 seconds
export STORAGE_FILE_PATH=/tmp/bosses.json
export STORAGE_FILE_FLUSH_INTERVAL=30s

# You can also use Redis as your data store
export STORAGE_REDIS_URI="redis://localhost"
export STORAGE_REDIS_FLUSH_INTERVAL=30s
```

## Prometheus Metrics

The HTTP server also exposes [Prometheus](https://prometheus.io/) metrics
at the `/metrics` endpoint.

Here's an example Docker setup for scraping these metrics:

```bash
cat << EOF > /tmp/prometheus.yml
scrape_configs:
- job_name: petronel
  scrape_interval: 10s
  metrics_path: /metrics
  static_configs:
  - targets: ["host.docker.internal:8080"]
EOF

docker run \
  --rm \
  -p 9090:9090 \
  -v /tmp/prometheus.yml:/etc/prometheus/prometheus.yml \
  prom/prometheus
```

> Note: `host.docker.internal` is [a DNS entry in Docker for Mac/Windows][host.docker.internal]
> that allows the container to connect to a service running on your host.

[host.docker.internal]: https://docs.docker.com/docker-for-mac/networking/#i-want-to-connect-from-a-container-to-a-service-on-the-host

Then open the Prometheus UI at <http://localhost:9090/graph>.

Here are some examples of queries you can run:
```
# Tweets per second (over the past 5 mins), grouped by boss name
sum by (name_en, name_ja) (rate(petronel_tweets_total[5m]))

# Tweets per second (over the past 5 mins), grouped by language
sum by (lang) (rate(petronel_tweets_total[5m]))
```

