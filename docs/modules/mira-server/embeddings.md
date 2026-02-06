# embeddings

Pending embeddings queue operations for background processing. Manages the queue of code chunks awaiting vector embedding generation.

## Key Type

`PendingEmbedding` - Represents a queued item with `id`, `project_id`, `file_path`, `chunk_content`, and `start_line`.

## Key Function

`get_pending_embeddings_sync()` - Fetches pending embeddings from the queue in batches for the background fast lane worker to process.

## Usage

The pending queue is primarily used for **incremental updates** (e.g., file watcher events after edits). The fast lane worker batches these chunks, generates embeddings via OpenAI, and stores them in sqlite-vec for semantic search.

Full project indexing embeds chunks inline when an embeddings client is available, and memory embeddings are generated at write time.
