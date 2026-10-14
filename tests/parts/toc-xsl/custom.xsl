<?xml version="1.0" encoding="UTF-8"?>
<xsl:stylesheet version="2.0"
                xmlns:xsl="http://www.w3.org/1999/XSL/Transform"
                xmlns:outline="http://wkhtmltopdf.org/outline"
                xmlns="http://www.w3.org/1999/xhtml">
  <xsl:output doctype-public="-//W3C//DTD XHTML 1.0 Strict//EN"
              doctype-system="http://www.w3.org/TR/xhtml1/DTD/xhtml1-strict.dtd"
              indent="yes" />
  <xsl:template match="outline:outline">
    <html>
      <head>
        <title>Custom TOC</title>
        <meta http-equiv="Content-Type" content="text/html; charset=utf-8" />
        <style>body { font-family: "DejaVu Serif"; } li { list-style: none; }</style>
      </head>
      <body>
        <h1>Custom TOC heading</h1>
        <ol><xsl:apply-templates select="outline:item/outline:item"/></ol>
      </body>
    </html>
  </xsl:template>
  <xsl:template match="outline:item">
    <li>
      <xsl:if test="@title!=''">
        <a href="{@link}">Entry: <xsl:value-of select="@title"/></a> @ p.<xsl:value-of select="@page"/>
      </xsl:if>
      <ol><xsl:apply-templates select="outline:item"/></ol>
    </li>
  </xsl:template>
</xsl:stylesheet>
