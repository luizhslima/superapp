import { buildApp } from "./app.js";
import { build } from "./providers.js";

const app = buildApp(build());
const port = Number(process.env.PORT ?? 8081);

app.listen({ host: "0.0.0.0", port }).catch((err) => {
  app.log.error(err);
  process.exit(1);
});
