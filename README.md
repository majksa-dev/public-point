# Public Point

A Rust Public Point Gateway built on top of custom gateway library.

[Crate API Documentation](https://majksa-dev.github.io/public-point/)

**Table of Contents**

- [Running](#running)
- [Gateway Configuration](#gateway-configuration)
- [Configuration file reference](#configuration-file-reference)
- [Example configuration](#example-configuration)

## Running

<!-- x-release-please-start-version -->

`docker run --rm -p 80:80 ghcr.io/majksa-dev/public-point:0.1.0`

<!-- x-release-please-end -->

## Gateway Configuration

| **ENV**          | **Description**                                              | **Default** |
| ---------------- | ------------------------------------------------------------ | ----------- |
| CONFIG_FILE      | Path to the configuration file                               |             |
| HOST             | HTTP host that the gateway will be exposed on.               | 127.0.0.1   |
| HTTP_PORT        | HTTP port that the gateway will be exposed on.               | 80          |
| HTTPS_PORT       | HTTPS port that the gateway will be exposed on.              | 443         |
| HEALTHCHECK_PORT | HTTP port that gateway healthcheck endpoint is available on. | 9000        |

## Configuration file reference

Json schema is available at: [./config.schema.json](https://raw.githubusercontent.com/majksa-dev/public-point/main/config.schema.json)

## Example configuration

```json
{
  "$schema": "https://raw.githubusercontent.com/majksa-dev/server-gateway/main/config.schema.json",
  "apps": {
    "app": {
      "upstream": {
        "host": "localhost",
        "port": 3005
      }
    }
  }
}
```
