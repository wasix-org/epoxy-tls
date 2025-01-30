export const Link: Component<{ href: string }, { children: any }> = function() {
	this.css = `
		text-decoration: underline !important;
	`;
	return <a href={this.href} rel="noopener noreferrer" target="_blank">{this.children}</a>
}

export const Git: Component<{ repo: string }, { children: any }> = function() {
	return <span><Link href={`https://github.com/${this.repo}`}>{this.children}</Link></span>;
}
