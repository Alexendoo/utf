/* eslint-disable no-param-reassign */
import initWasm, { Collector, SearchResults } from "./wasm";

class SearchPool {
    /**
     * @param {Array<Worker>} pool
     */
    constructor(pool) {
        this.pool = pool;
        this.epoch = 0;
    }

    clear() {
        this.epoch += 1;
        this.pool.forEach((worker) => {
            worker.onmessage = null;
        });
    }

    /**
     * @param {string} pattern
     * @param {(results: SearchResults) => void} callback
     */
    search(pattern, callback) {
        this.epoch += 1;

        if (pattern === "") {
            callback(SearchResults.empty());
            return;
        }

        const collector = new Collector(this.pool.length);

        this.pool.forEach((worker) => {
            worker.postMessage({ pattern, epoch: this.epoch });

            worker.onmessage = ({ data }) => {
                if (data.epoch !== this.epoch) {
                    // Old event, ignore it
                    return;
                }

                const done = collector.collect(new Uint8Array(data.buffer));

                if (done) {
                    const results = collector.build();

                    callback(results);
                }
            };
        });
    }
}

function createWorker(number) {
    return new Worker(new URL("../worker.js", import.meta.url), {
        name: `worker:${number}`,
    });
}

async function loadWorker(worker, initialData) {
    await new Promise((resolve, reject) => {
        worker.postMessage(initialData);

        worker.onmessage = resolve;
        worker.onerror = reject;
    });

    worker.onmessage = null;
    worker.onerror = null;
}

export default async function loadPool() {
    const numWorkers = navigator.hardwareConcurrency;

    const workers = Array.from({ length: numWorkers }, (_, workerNumber) =>
        createWorker(workerNumber),
    );

    const module = await initWasm();

    await Promise.all(
        workers.map((worker, workerNumber) =>
            loadWorker(worker, {
                workerNumber,
                numWorkers,
                module,
            }),
        ),
    );

    return new SearchPool(workers);
}
