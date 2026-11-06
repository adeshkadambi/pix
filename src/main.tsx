/* @refresh reload */
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { render } from "solid-js/web";
import "./styles/base.css";
import { Companion } from "./windows/companion/Companion";
import { Hud } from "./windows/hud/Hud";
import { TabPanel } from "./windows/tabpanel/TabPanel";

// All three windows load this page and pick their view by window label.
const label = getCurrentWebviewWindow().label;
document.documentElement.dataset.window = label;

const views = { companion: Companion, hud: Hud, tabpanel: TabPanel } as const;
const View = views[label as keyof typeof views] ?? Companion;

render(() => <View />, document.getElementById("root")!);
