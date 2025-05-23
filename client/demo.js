import { EpoxyClient, init } from "./dist/epoxy.js";

await init();

const client = new EpoxyClient();

await client.fetch();
