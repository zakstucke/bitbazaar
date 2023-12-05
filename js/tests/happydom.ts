import { GlobalRegistrator } from "@happy-dom/global-registrator";

// The console stuff fixes test logging when happy-dom enabled:
// https://github.com/oven-sh/bun/issues/6044
const oldConsole = console;
GlobalRegistrator.register();
window.console = oldConsole;
