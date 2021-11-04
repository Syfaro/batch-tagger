# batch-tagger

Add tags or remove tags from many submissions on FurAffinity and Weasyl.

## Usage

```bash
# Load a copy of every submission for local evaluation
./batch-tagger --furaffinity-cookie-a cookie_a --furaffinity-cookie-b cookie_b --weasyl-api-key api_key --furaffinity-user your-user --weasyl-user your-user load-submissions
# Find which submissions you want to re-tag
./batch-tagger --furaffinity-cookie-a cookie_a --furaffinity-cookie-b cookie_b --weasyl-api-key api_key --furaffinity-user your-user --weasyl-user your-user query-tags --search "tag1 -not-tag2"
# Look at a dry-run of the changes that will be performed
./batch-tagger --furaffinity-cookie-a cookie_a --furaffinity-cookie-b cookie_b --weasyl-api-key api_key --furaffinity-user your-user --weasyl-user your-user apply-tags --dry-run --search "tag1 -not-tag2" --tags "new-tag -remove-tag3"
# Perform changes for real
./batch-tagger --furaffinity-cookie-a cookie_a --furaffinity-cookie-b cookie_b --weasyl-api-key api_key --furaffinity-user your-user --weasyl-user your-user apply-tags --search "tag1 -not-tag2" --tags "new-tag -remove-tag3"
```
