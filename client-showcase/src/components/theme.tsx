import { css, type Component } from "dreamland/core";
import settings from "../store";

const Theme: Component = function() {
	return <a on:click={(e: PointerEvent) => { 
        e.preventDefault(); 
        settings.theme = settings.theme === "light" ? "dark" : "light" 
    }} href="#"><i>{use(settings.theme)}</i></a>;
}
Theme.style = css`
	:scope {
		text-decoration: underline !important;
	}
`

export default Theme;