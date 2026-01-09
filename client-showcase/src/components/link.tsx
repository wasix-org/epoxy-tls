import { css, type Component, type ComponentChild } from "dreamland/core";

const Link: Component<{ href: string, children: ComponentChild }> = function(cx) {
	return <a href={this.href} rel="noopener noreferrer" target="_blank">{cx.children}</a>
};

Link.style = css`
    text-decoration: underline !important;
`;

export default Link;