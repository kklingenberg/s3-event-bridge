# S3 Event Bridge

This is a wrapper used to build AWS Lambdas which react to S3 events. Lambdas
built using this are defined in terms of other programs, which in turn can be
described in these four statements:

1. They can be invoked with a shell instruction.
2. They exit when they're done with their work.
3. Their inputs are files, placed in paths starting from some root folder.
4. Their outputs are files, placed in paths starting from the same root folder
   as the inputs.

This setup allows for the construction of lambda functions that skip over the
semantics of AWS Lambda invocation and simply operate on files, and which use
whichever runtime the user needs them to use.

## Invocation sequence

For each lambda invocation, the following steps happen in order:

1. The relevant input files are pulled from S3 and copied into a temporary
   folder. A signature snapshot of each file is taken.
2. The handler program is invoked. The temporary folder is passed to the handler
   program as an environment variable.
3. After the handler program exits, another signature snapshot is taken from the
   files in the temporary folder. Each signature difference from the one in step
   2 causes the event bridge to upload the files to S3 (to the same bucket, or a
   new one).

## Configuration

Configuration is achieved via the following environment variables:

- `MATCH_KEY` is a limited pattern of keys to cause triggers. The wildcard `*`
  may be used to match any word without separators (/). If omitted, any key will
  cause a trigger.
- `PULL_PARENT_DIRS` is a number representing the parent directories to be
  pulled from S3 to serve as inputs, starting from the folder where the matching
  key is located. `0` means to pull just the folder containing the key. A
  negative number is interpreted to mean the whole bucket. This parameter is
  relevant to consider the structure of outputs, since they will be located
  somewhere in the hierarchy starting from this folder. Default value is `0`.
- `PULL_MATCH_KEYS` is a comma-separated list of patterns used to select files
  being pulled to serve as inputs. If omitted, it will default to matching all
  files. If not omitted, it's up to the user to include the same pattern as
  `MATCH_KEY`, or to exclude it if the triggering key is not meant to be pulled.
- `TARGET_BUCKET` is the bucket name that will receive outputs. If omitted, it
  will default to the same bucket as the one specified in the original event.
- `HANDLER_COMMAND` is the shell expression that starts the command that handles
  files and does the actual work.
- `ROOT_FOLDER_VAR` is the name of the environment variable that will be
  populated for the handler program, containing the path to the temporary folder
  which contains the inputs and outputs. Defaults to `ROOT_FOLDER`.
- `BUCKET_VAR` is the name of the environment variable that will be populated
  for the handler program, containing the name of the bucket from which files
  are being pulled to act as inputs. Defaults to `BUCKET`.
- `KEY_PREFIX_VAR` is the name of the environment variable that will be
  populated for the handler program, containing object key prefix used to select
  input files to be pulled, to act as inputs. Defaults to `KEY_PREFIX`.

## Deployment

This is mostly intended to be deployed as an entrypoint in a Docker image,
alongside the dependencies and runtimes of the handler program. For example, to
run a hypothetical python script `handler.py` as a lambda function, you could
write something like this:

```dockerfile
FROM python:3.11

WORKDIR /app
COPY handler.py ./

# Install the event bridge
RUN curl -L -o /usr/bin/bootstrap \
    https://github.com/kklingenberg/s3-event-bridge/releases/download/v0.2.0/bootstrap && \
    chmod +x /usr/bin/bootstrap

# Provide the instruction to be run for each event
ENV HANDLER_COMMAND="python handler.py"

ENTRYPOINT ["/usr/bin/bootstrap"]
```

In this example, it'll be up to the script `handler.py` to properly consider
files using the environment variable `ROOT_FOLDER` as base. For example, if such
a script expected a file named `inputs.json`, it would have to read it similarly
to:

```python
import json
import os
from pathlib import Path

base_path = Path(os.getenv("ROOT_FOLDER", "."))

with open(base_path / "inputs.json") as f:
    input_data = json.load(f)
```
