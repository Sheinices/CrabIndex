// Turns headings like "## GET /api/v1.0/conf" into a coloured method badge + code path.
const METHOD = /^(GET|POST|PUT|PATCH|DELETE|ANY)\s+(\S.*)$/

function visit(node, fn) {
  fn(node)
  if (node.children) node.children.forEach((child) => visit(child, fn))
}

export default function httpMethodBadges() {
  return (tree) => {
    visit(tree, (node) => {
      if (node.type !== 'heading' || node.children?.length !== 1 || node.children[0].type !== 'text') return
      const match = METHOD.exec(node.children[0].value.trim())
      if (!match) return
      const [, method, path] = match
      node.children = [
        {
          type: 'emphasis',
          data: { hName: 'span', hProperties: { className: ['http-method', `http-method--${method.toLowerCase()}`] } },
          children: [{ type: 'text', value: method }],
        },
        { type: 'text', value: ' ' },
        { type: 'inlineCode', value: path },
      ]
    })
  }
}
