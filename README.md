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

1. The events are batched into _execution groups_ and each group is evaluated
   with an execution criterion.
2. For each execution group that passes the corresponding criterion, the
   relevant input files are pulled from S3 and copied into a temporary folder. A
   signature snapshot of each file is taken.
3. For each group, the handler program is invoked. The temporary folder is
   passed to the handler program as an environment variable.
4. For each group, after the handler program exits, another signature snapshot
   is taken from the files in the temporary folder. Each signature difference
   from the one in step 2 causes the event bridge to upload the files to S3 (to
   the same bucket, or a new one).

The intended use involves bridging S3 events through SQS queues (or SNS topics
connected to SQS queues). The SQS queue is in turn connected as a Lambda trigger. 

```mermaid
graph TD;
    S3Source[("S3\nsource bucket")]-- "0. Emit native S3 event" -->SQS;
    SQS-- "1. Receive trigger and\nassemble batches of events" -->Lambda("Lambda\ns3-event-bridge");
    Lambda-- "2. Download objects as inputs" -->S3Source;
    Lambda-- "3. Invoke handler program" -->Lambda;
    Lambda-- 4. Upload outputs -->S3Target[("S3\ntarget bucket")];
```

## Configuration

Configuration is achieved via the following environment variables:

- `MATCH_KEY` is a limited regex pattern of keys to cause triggers, defined in
  terms of the [regex crate's
  syntax](https://docs.rs/regex/latest/regex/#syntax). If omitted, any key will
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
- `EXECUTION_FILTER_EXPR` and `EXECUTION_FILTER_FILE` define either a
  [jq](https://stedolan.github.io/jq/) expression or the path to a file
  containing a jq expression (UTF-8 encoded), that will be executed for the set
  of S3 objects pre-selected to be pulled (before being filtered according to
  `PULL_MATCH_KEYS`). The expression is passed an array of these objects (as
  defined by the [Object
  API](https://docs.aws.amazon.com/AmazonS3/latest/API/API_Object.html)), and
  must evaluate to a single value. **If the value it produces is not explicitly
  `false`, it will continue with the execution of the handler program**. Only
  one of these variables may be defined, and if both are omitted or left blank,
  they default to the equivalent of a constant `empty` jq expression.
- `TARGET_BUCKET` is the bucket name that will receive outputs. If omitted, it
  will default to the same bucket as the one specified in the original event.
- `ROOT_FOLDER_VAR` is the name of the environment variable that will be
  populated for the handler program, containing the path to the temporary folder
  which contains the inputs and outputs. Defaults to `ROOT_FOLDER`.
- `BUCKET_VAR` is the name of the environment variable that will be populated
  for the handler program, containing the name of the bucket from which files
  are being pulled to act as inputs. Defaults to `BUCKET`.
- `KEY_PREFIX_VAR` is the name of the environment variable that will be
  populated for the handler program, containing object key prefix used to select
  input files to be pulled, to act as inputs. Defaults to `KEY_PREFIX`.

Apart from the configuration variables, the AWS Lambda bootstrap binary needs to
receive the handler command expression as its argument (e.g. if the bootstrap
binary is place in the current directory, `./lambda-bootstrap ls` would execute
`ls` as the handler).

### A small note on map-reduce

This whole wrapper thing was made to accomplish the goal of enabling simple
file-based, script-like programs to run parallel, coordinated tasks in AWS
Lambda. The **parallel execution** part of the requirement comes for free: if
you model inputs as files in S3, and bind S3 events (through an SQS queue) to
Lambda invocations, you get many instances of the program running on-demand as
soon as inputs appear. Moreover, given that outputs are also files, an also
"for-free" capability is that of **composition**, in that outputs of some
programs can directly trigger the execution of other programs (even themselves,
in a fixed-point-combinatory fashion).

The **coordination** part of the requirements is a bit more involved. To keep it
contained within this single enabling component, and to not involve additional
services, the `EXECUTION_FILTER_*` configuration variables were added. They
serve the requirement of coordination under the assumption that a reduce-kind of
event occurs when something about the state of the S3 bucket happens. For
example: assuming that executing a "reduce" phase is only relevant once all of
the many executions of the "map" phase are complete, and given that the "map"
phase produces outputs as files, you could decide to run the "reduce" phase once
every input file is matched by a corresponding output file. The task is then to
write said criterion as a jq expression. This could look like the following:

```jq
group_by(.Key | split("/") | .[:-1]) | all(
  any(.Key | split("/") | .[-1] == "input.txt") and
  any(.Key | split("/") | .[-1] == "output.txt")
)
```

This example doesn't consider any other properties of the objects, which might
serve to catch weird error cases in which a reduce phase would not be
wanted. For example, if the `output.txt` file is 0 bytes in size, you might
consider that an error (discernible through the `Size` property).

In case no automatic coordination phase of a data processing pipeline is needed,
the `EXECUTION_FILTER_*` variables may be left undefined.

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
ARG bootstrap_url=https://github.com/kklingenberg/s3-event-bridge/releases/download/v0.5.0/lambda-bootstrap-linux-x86_64
RUN set -ex ; \
    curl "${bootstrap_url}" -L -o /usr/bin/bootstrap ; \
    chmod +x /usr/bin/bootstrap

ENTRYPOINT ["/usr/bin/bootstrap"]
# Provide the instruction to be run for each event
CMD ["python", "handler.py"]
```

In this example, it'll be up to the script `handler.py` to properly consider
files using the environment variable `ROOT_FOLDER` as base. For example, if such
a script expected a file named `input.txt`, it would have to read it similarly
to:

```python
import os
from pathlib import Path

base_path = Path(os.getenv("ROOT_FOLDER", "."))

with open(base_path / "input.txt") as f:
    input_data = f.read()

def process(data):
    result = ""
    # ...
    return result

output_data = process(input_data)

with open(base_path / "output.txt", "w") as f:
    f.write(output_data)
```

Also, and for the time being, the integration mechanism is limited to an SQS
queue triggering a Lambda function, with said queue only being fed events from
S3 buckets. This may change in the future, possibly to consider other kinds of
integrations with S3 (e.g. direct invocation, SNS publish/subscribe, etc.).

## Usage as CLI wrapper

Included in the release artifacts there's also a `command` utility that operates
just like the lambda bootstrap binary, but as a one-shot utility for CLI
programs.

```bash
env BUCKET=some-bucket \
    KEY_PREFIX=some/prefix/of/keys/to/pull \
    ./command python handler.py
```

## Usage as a standalone SQS queue consumer

Also included in the release artifacts there's a `sqs-consumer` utility that
operates just like the other binaries, but invoking the specified handler
program continuously every time a batch of SQS messages is received. The utility
expects additional parameters that indicate the queue to consume and the way to
consume it.

```bash
env SQS_QUEUE_URL=https://sqs.<region>.amazonaws.com/<account-id>/<queue-name> \
    SQS_VISIBILITY_TIMEOUT=600 \
    SQS_MAX_NUMBER_OF_MESSAGES=10 \
    ./sqs-consumer python handler.py
```

## Usage as glue for other AWS services

> :warning: This isn't the intended use case for this utility, as the resulting
> docker image will be bloated and cause unnecessary costs. I recommend to just
> implement the AWS API calls using a
> [runtime](https://docs.aws.amazon.com/lambda/latest/dg/lambda-runtimes.html)
> and the corresponding [AWS SDK](https://aws.amazon.com/developer/tools/).

A common (and in fashion, given all the craze about training ML models)
integration you would want is between S3 events and AWS Batch jobs. You could
achieve it using this utility run in an AWS Lambda that acts like the bridge
between both services, by installing [AWSCLI](https://aws.amazon.com/cli/) in
the docker image:

```dockerfile
FROM debian:stable-slim

ARG bootstrap_url=https://github.com/kklingenberg/s3-event-bridge/releases/download/v0.5.0/lambda-bootstrap-linux-x86_64

RUN set -ex ; \
    apt-get update ; \
    apt-get install -y groff less curl unzip ; \
    curl https://awscli.amazonaws.com/awscli-exe-linux-x86_64.zip -o awscliv2.zip ; \
    unzip awscliv2.zip ; \
    ./aws/install ; \
    rm -r awscliv2.zip ./aws ; \
    curl "${bootstrap_url}" -L -o /usr/bin/bootstrap ; \
    chmod +x /usr/bin/bootstrap ; \
    apt-get purge -y curl unzip ; \
    apt-get autoremove -y ; \
    apt-get clean ; \
    rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY command.sh ./

# Don't pull any file from S3, since they're not needed
ENV PULL_MATCH_KEYS="^$"

ENTRYPOINT ["/usr/bin/bootstrap"]
CMD ["bash", "command.sh"]
```

And a script file `command.sh` like this:

```bash
set -ex

cat > "${ROOT_FOLDER}/batch_job_container_overrides.json" <<EOF
{
  "environment": [
    {
      "name": "BUCKET",
      "value": "${BUCKET}"
    },
    {
      "name": "KEY_PREFIX",
      "value": "${KEY_PREFIX}"
    }
  ]
}
EOF

aws batch submit-job \
    --job-name <some-job-name> \
    --job-queue <some-job-queue> \
    --job-definition <some-job-definition> \
    --container-overrides "file://${ROOT_FOLDER}/batch_job_container_overrides.json" \
    --output json \
    > "${ROOT_FOLDER}/batch_job.json"
```
