import { createStore } from "dreamland/core";

const settings =  createStore({
	epoxyVersion: "",
	wispServer: "wss://wisp.mercurywork.shop/",
	theme: window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light",
}, { ident: "epoxy-showcase-settings", backing: "localstorage", autosave: "auto" })

// @ts-ignore
window.settings = settings;
export default settings;