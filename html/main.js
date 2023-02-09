var router ;
var app ;
let wd = new WikiData() ;
var user ;



function set_user_data(d) {
    user = d.user;
    if ( user==null ) {
        $("#user_greeting").text("");
        $("#login_logout").html("<span tt='login'></span>").attr("href","/auth/login");
    } else {
        $("#user_greeting").text(user.username);
        $("#login_logout").html("<span tt='logout'></span>").attr("href","/auth/logout");
    }
}

$(document).ready(function(){
    vue_components.toolname = 'gulp' ;
    Promise.all ( [
            vue_components.loadComponents ( ['tool-translate','wd-link',//'autodesc','wikidatamap','wd-date','tool-navbar','commons-thumbnail',
                'vue_components/main_page.html',
                'vue_components/list_page.html',
                'vue_components/list.html',
                ] ) ,
            new Promise(function(resolve, reject) {
                fetch("/auth/info")
                    .then((response) => response.json())
                    .then((d)=>set_user_data(d))
                    .then(resolve)
            } )
    ] ) .then ( () => {
        current_language = tt.language ;
        wd_link_wd = wd ;
        tt.addILdropdown ( '#tooltranslate_wrapper' ) ;

        const routes = [
          { path: '/', component: MainPage },
          { path: '/list/:list_id', component: ListPage , props:true },
/*
          { path: '/group', component: CatalogGroup , props:true },
          { path: '/group/:key', component: CatalogGroup , props:true },
*/
        ] ;

        router = new VueRouter({routes}) ;
        app = new Vue ( { router } ) .$mount('#app') ;
        setTimeout(function(){
            tt.updateInterface($('body')) ;
        },100);
    } ) ;

} ) ;
