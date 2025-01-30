export const settings: Stateful<{
	epoxyVersion: string,
	wispServer: string,

	theme: "light" | "dark",
}> = $store({
	epoxyVersion: "",
	wispServer: "wss://wisp.mercurywork.shop/",

	theme: window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light",
}, { ident: "epoxy-showcase-settings", backing: "localstorage", autosave: "auto" })

// @ts-ignore
window.settings = settings;
