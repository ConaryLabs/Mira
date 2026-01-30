# embeddings

Pending embeddings queue operations for background processing. Manages the queue of text chunks awaiting vector embedding generation.

## Key Type

`PendingEmbedding` - Represents a queued item with `id`, `project_id`, `file_path`, `chunk_content`, and `start_line`.

## Key Function

`get_pending_embeddings_sync()` - Fetches pending embeddings from the queue in batches for the background fast lane worker to process.

## Usage

When memories are stored or code is indexed, chunks are added to the pending embeddings queue. The background fast lane worker picks them up, generates vector embeddings via the Gemini embedding API, and stores them in sqlite-vec for semantic search.
